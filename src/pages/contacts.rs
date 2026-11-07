//! Virtualised contact list backed by the CareLibre server API.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use arc_swap::ArcSwap;
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Layout, Margin, Painter, Pos2, Rect,
    RichText, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};

use crate::store::{ContactCounts, ContactCursor, ContactFilter, ContactKind, ContactRow};
use crate::theme;

const ROW_HEIGHT: f32 = 54.0;
const HEADER_HEIGHT: f32 = 28.0;
pub const PAGE_SIZE: usize = 200;
pub const LOAD_MORE_SIZE: usize = 500;
const PREFETCH_ROWS: usize = 40;
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(120);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContactsAction {
    SearchChanged,
    LoadMore,
    OpenContact(String),
    GoToSettings,
    StartIntake,
}

pub struct ContactsState {
    pub filter: ContactFilter,
    pub search: String,
    pub rows: ArcSwap<Vec<ContactRow>>,
    pub total: usize,
    pub counts: ContactCounts,
    pub next_cursor: Option<ContactCursor>,
    pub generation: u64,
    pub search_due: Option<Instant>,
    pub loading: bool,
    pub error: Option<String>,
    pub selected: Option<String>,
    pub last_query_ms: Option<f32>,
    pub needs_more: bool,
}

impl ContactsState {
    pub fn new(filter: ContactFilter) -> Self {
        Self {
            filter,
            search: String::new(),
            rows: ArcSwap::from_pointee(Vec::new()),
            total: 0,
            counts: ContactCounts::default(),
            next_cursor: None,
            generation: 0,
            search_due: None,
            loading: false,
            error: None,
            selected: None,
            last_query_ms: None,
            needs_more: false,
        }
    }

    pub fn loaded(&self) -> usize {
        self.rows.load().len()
    }

    pub fn reset(&self) {
        self.rows.store(Arc::new(Vec::new()));
    }
}

pub fn show(
    ui: &mut Ui,
    state: &mut ContactsState,
    signed_in: bool,
    server_error: Option<&str>,
) -> Option<ContactsAction> {
    if !signed_in {
        return sign_in_card(ui);
    }

    if let Some(error) = server_error {
        message_card(ui, "Server unavailable", error, theme::DANGER);
        return None;
    }

    header(ui, state);
    ui.add_space(12.0);
    stat_chips(ui, state);
    ui.add_space(12.0);
    let mut action = search_row(ui, state);

    if let Some(error) = &state.error {
        ui.add_space(8.0);
        message_card(ui, "Query failed", error, theme::DANGER);
    }

    ui.add_space(8.0);

    if state.counts.all == 0 && state.loading && state.error.is_none() {
        message_card(
            ui,
            "Loading contacts",
            "Fetching the first page from the CareLibre server.",
            theme::MUTED,
        );
    } else if state.counts.all == 0 && state.search.trim().is_empty() && state.error.is_none() {
        empty_card(ui, &mut action);
    } else if state.total == 0 && !state.search.trim().is_empty() {
        message_card(
            ui,
            "No matches",
            &format!("No contacts match “{}”.", state.search.trim()),
            theme::MUTED,
        );
    } else {
        contacts_table(ui, state, &mut action);
    }

    action
}

fn header(ui: &mut Ui, state: &ContactsState) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(state.filter.label()).font(theme::strong_font(26.0)));
                theme::pill(ui, &format_count(state.total), theme::ACCENT);
            });
            ui.label(
                RichText::new(
                    "Live contact data from the CareLibre server, fetched by a background worker.",
                )
                .color(theme::MUTED)
                .size(12.5),
            );
        });

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            theme::pill(ui, "SERVER", theme::ACCENT);
        });
    });
}

fn stat_chips(ui: &mut Ui, state: &ContactsState) {
    ui.horizontal(|ui| {
        chip(ui, "All contacts", state.counts.all, theme::TEXT);
        chip(ui, "Clients", state.counts.clients, theme::ACCENT_BLUE);
        chip(ui, "Workers", state.counts.workers, theme::ACCENT);
        chip(ui, "Dual role", state.counts.dual, theme::WARNING);
        chip(ui, "Other", state.counts.other, theme::FAINT);
    });
}

fn chip(ui: &mut Ui, label: &str, value: usize, colour: Color32) {
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(11, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format_count(value))
                        .color(colour)
                        .font(theme::strong_font(14.0)),
                );
                ui.label(RichText::new(label).color(theme::MUTED).size(10.5));
            });
        });
}

fn search_row(ui: &mut Ui, state: &mut ContactsState) -> Option<ContactsAction> {
    let mut action = None;

    ui.horizontal(|ui| {
        let response = ui.add_sized(
            [400.0, 34.0],
            egui::TextEdit::singleline(&mut state.search)
                .hint_text("Search name, email or phone…")
                .margin(Margin::symmetric(10, 8)),
        );
        if response.changed() {
            state.search_due = Some(Instant::now() + SEARCH_DEBOUNCE);
            state.selected = None;
            action = Some(ContactsAction::SearchChanged);
        }

        if !state.search.is_empty()
            && ui
                .add(
                    egui::Button::new(RichText::new("Clear").color(theme::FAINT).size(11.0))
                        .frame(false),
                )
                .clicked()
        {
            state.search.clear();
            state.search_due = Some(Instant::now());
            action = Some(ContactsAction::SearchChanged);
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let loaded = state.rows.load().len();
            let mut text = format!("{loaded} loaded · {} total", state.total);
            if state.loading {
                text.push_str(" · updating");
            }
            ui.label(RichText::new(text).color(theme::FAINT).size(10.5));
            if let Some(ms) = state.last_query_ms {
                ui.label(
                    RichText::new(format!("{ms:.1} ms"))
                        .color(theme::FAINT)
                        .size(10.5)
                        .monospace(),
                );
            }
        });
    });

    action
}

fn contacts_table(ui: &mut Ui, state: &mut ContactsState, action: &mut Option<ContactsAction>) {
    table_header(ui);

    let total = state.total;
    let mut clicked_id: Option<String> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, ROW_HEIGHT, total, |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let snapshot = state.rows.load_full();
            let loaded = snapshot.len();
            let range_end = range.end;

            for index in range {
                if let Some(row) = snapshot.get(index) {
                    if row_row(ui, row, index, state.selected.as_deref()) {
                        clicked_id = Some(row.id.clone());
                    }
                } else {
                    placeholder_row(ui, index);
                }
            }

            if loaded < total
                && state.next_cursor.is_some()
                && !state.loading
                && state.search_due.is_none()
                && range_end + PREFETCH_ROWS >= loaded
            {
                state.needs_more = true;
            }
        });

    if let Some(id) = clicked_id {
        state.selected = Some(id.clone());
        if action.is_none() {
            *action = Some(ContactsAction::OpenContact(id));
        }
    }

    if action.is_none()
        && state.needs_more
        && !state.loading
        && state.search_due.is_none()
        && state.next_cursor.is_some()
    {
        state.needs_more = false;
        *action = Some(ContactsAction::LoadMore);
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let loaded = state.rows.load().len();
        ui.label(
            RichText::new(format!(
                "Showing {} of {} {}. Scroll to load more.",
                format_count(loaded),
                format_count(state.total),
                state.filter.label().to_lowercase(),
            ))
            .color(theme::FAINT)
            .size(10.0),
        );
    });
}

fn table_header(ui: &mut Ui) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), HEADER_HEIGHT), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, theme::SIDEBAR);
    painter.hline(
        rect.x_range(),
        rect.bottom(),
        Stroke::new(1.0, theme::BORDER),
    );

    let width = rect.width();
    let y = rect.center().y;
    let font = FontId::proportional(10.5);
    let colour = theme::FAINT;

    painter.text(
        pos2(rect.left() + 58.0, y),
        Align2::LEFT_CENTER,
        "NAME",
        font.clone(),
        colour,
    );
    painter.text(
        pos2(rect.left() + width * 0.36, y),
        Align2::LEFT_CENTER,
        "CONTACT",
        font.clone(),
        colour,
    );
    painter.text(
        pos2(rect.left() + width * 0.60, y),
        Align2::LEFT_CENTER,
        "DIVISION",
        font.clone(),
        colour,
    );
    painter.text(
        pos2(rect.left() + width * 0.79, y),
        Align2::CENTER_CENTER,
        "ROLE",
        font.clone(),
        colour,
    );
    painter.text(
        pos2(rect.left() + width * 0.91, y),
        Align2::CENTER_CENTER,
        "STATUS",
        font,
        colour,
    );
}

fn row_row(ui: &mut Ui, row: &ContactRow, index: usize, selected: Option<&str>) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());

    let is_selected = selected == Some(row.id.as_str());
    let fill = if is_selected {
        theme::accent_soft(20)
    } else if response.hovered() {
        theme::SURFACE
    } else if index % 2 == 1 {
        Color32::from_rgb(12, 19, 32)
    } else {
        Color32::TRANSPARENT
    };

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, fill);
    painter.hline(
        rect.x_range(),
        rect.bottom(),
        Stroke::new(1.0, Color32::from_rgb(24, 34, 53)),
    );

    let width = rect.width();
    let centre_y = rect.center().y;
    let kind_colour = kind_colour(row.kind);

    // Avatar
    let avatar_centre = pos2(rect.left() + 30.0, centre_y);
    painter.circle_filled(
        avatar_centre,
        14.0,
        Color32::from_rgba_unmultiplied(kind_colour.r(), kind_colour.g(), kind_colour.b(), 45),
    );
    painter.text(
        avatar_centre,
        Align2::CENTER_CENTER,
        initials(&row.name),
        FontId::proportional(10.5),
        kind_colour,
    );

    // Name
    let name_x = rect.left() + 58.0;
    let name_width = width * 0.34 - 58.0 - 12.0;
    painter.text(
        pos2(name_x, centre_y),
        Align2::LEFT_CENTER,
        truncate(&row.name, (name_width / 6.7) as usize),
        theme::strong_font(13.0),
        theme::TEXT,
    );

    // Contact details
    let contact_x = rect.left() + width * 0.36;
    let contact_width = width * 0.22 - 14.0;
    let email = row.email.as_deref().unwrap_or("—");
    painter.text(
        pos2(contact_x, centre_y - 7.0),
        Align2::LEFT_CENTER,
        truncate(email, (contact_width / 5.5) as usize),
        FontId::proportional(10.5),
        theme::MUTED,
    );
    painter.text(
        pos2(contact_x, centre_y + 7.0),
        Align2::LEFT_CENTER,
        row.phone.as_deref().unwrap_or("—"),
        FontId::monospace(9.5),
        theme::FAINT,
    );

    // Division
    let division_x = rect.left() + width * 0.60;
    painter.text(
        pos2(division_x, centre_y),
        Align2::LEFT_CENTER,
        truncate(
            row.division.as_deref().unwrap_or("—"),
            (width * 0.17 / 5.8) as usize,
        ),
        FontId::proportional(11.0),
        theme::MUTED,
    );

    // Role and status pills
    pill(
        painter,
        pos2(rect.left() + width * 0.79, centre_y),
        row.kind.label(),
        kind_colour,
    );
    pill(
        painter,
        pos2(rect.left() + width * 0.91, centre_y),
        &row.status,
        status_colour(&row.status),
    );

    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            ui.is_enabled(),
            format!("{}, {} — {}", row.name, row.kind.label(), row.status),
        )
    });

    response.clicked()
}

fn placeholder_row(ui: &mut Ui, index: usize) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    let painter = ui.painter();
    if index % 2 == 1 {
        painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(12, 19, 32));
    }
    painter.text(
        pos2(rect.left() + 58.0, rect.center().y),
        Align2::LEFT_CENTER,
        "Loading local rows…",
        FontId::proportional(11.0),
        theme::FAINT,
    );
}

fn sign_in_card(ui: &mut Ui) -> Option<ContactsAction> {
    let mut action = None;
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(22))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Sign in to load contacts").font(theme::strong_font(18.0)));
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Contacts are read directly from the CareLibre server. Sign in with your organisation account in Settings to continue.",
                )
                .color(theme::MUTED)
                .size(12.0),
            );
            ui.add_space(12.0);
            if ui.add(theme::primary_button("Go to Settings")).clicked() {
                action = Some(ContactsAction::GoToSettings);
            }
        });
    action
}

fn empty_card(ui: &mut Ui, action: &mut Option<ContactsAction>) {
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(22))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("No contacts on the server yet").font(theme::strong_font(18.0)));
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Create the first contact with a guided intake. Completed intakes are saved to the server.",
                )
                .color(theme::MUTED)
                .size(12.0),
            );
            ui.add_space(12.0);
            if ui.add(theme::primary_button("Start intake")).clicked() {
                *action = Some(ContactsAction::StartIntake);
            }
        });
}

fn message_card(ui: &mut Ui, title: &str, message: &str, colour: Color32) {
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(20))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(title)
                    .color(colour)
                    .font(theme::strong_font(15.0)),
            );
            ui.add_space(4.0);
            ui.label(RichText::new(message).color(theme::MUTED).size(12.0));
        });
}

fn kind_colour(kind: ContactKind) -> Color32 {
    match kind {
        ContactKind::Client => theme::ACCENT_BLUE,
        ContactKind::Worker => theme::ACCENT,
        ContactKind::Dual => theme::WARNING,
        ContactKind::Other => theme::FAINT,
    }
}

fn status_colour(status: &str) -> Color32 {
    match status {
        "Active" => theme::SUCCESS,
        "Onboarding" => theme::WARNING,
        "Inactive" => theme::FAINT,
        _ => theme::MUTED,
    }
}

fn initials(name: &str) -> String {
    let parts: Vec<&str> = name
        .split_whitespace()
        .filter(|part| !part.ends_with('.'))
        .collect();
    match (parts.first(), parts.last()) {
        (Some(first), Some(last)) if parts.len() > 1 => format!(
            "{}{}",
            first.chars().next().unwrap_or('?'),
            last.chars().next().unwrap_or('?')
        )
        .to_uppercase(),
        (Some(first), _) => first
            .chars()
            .next()
            .unwrap_or('?')
            .to_string()
            .to_uppercase(),
        _ => "?".to_owned(),
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if max_chars < 4 {
        return text.to_owned();
    }
    let mut chars = text.chars();
    let collected: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        let mut shortened: String = collected
            .chars()
            .take(max_chars.saturating_sub(1))
            .collect();
        shortened.push('…');
        shortened
    } else {
        collected
    }
}

fn pill(painter: &Painter, centre: Pos2, text: &str, colour: Color32) {
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(10.0), colour);
    let size = galley.size() + Vec2::new(14.0, 6.0);
    let rect = Rect::from_center_size(centre, size);
    painter.rect(
        rect,
        CornerRadius::same(9),
        Color32::from_rgba_unmultiplied(colour.r(), colour.g(), colour.b(), 26),
        Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(colour.r(), colour.g(), colour.b(), 60),
        ),
        StrokeKind::Inside,
    );
    painter.galley(rect.center() - galley.size() / 2.0, galley, colour);
}

fn format_count(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}
