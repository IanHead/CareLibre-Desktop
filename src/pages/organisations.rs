//! External organisation list, creation and editing.

use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea, Sense, Stroke,
    StrokeKind, Ui,
};

use crate::organisation::{
    ApiOrganisation, ApiOrganisationWrite, ORGANISATION_KINDS, OrganisationStatusFilter,
    kind_label, optional,
};
use crate::theme;

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrganisationsAction {
    SearchChanged,
    FilterChanged,
    Reload,
    New,
    Edit(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrganisationEditorAction {
    Save,
    Cancel,
    Delete,
}

/// The state behind one organisation column. Creating uses a `new` editor;
/// editing uses the record id so the same organisation is focused rather than
/// opened twice.
pub struct OrganisationEditorState {
    pub record_id: Option<String>,
    pub form: OrganisationForm,
    pub pending: bool,
    pub error: Option<String>,
    pub confirm_delete: bool,
    pub delete_pending: bool,
}

impl OrganisationEditorState {
    pub fn new() -> Self {
        Self {
            record_id: None,
            form: OrganisationForm::new(),
            pending: false,
            error: None,
            confirm_delete: false,
            delete_pending: false,
        }
    }

    pub fn edit(organisation: &ApiOrganisation) -> Self {
        Self {
            record_id: Some(organisation.id.clone()),
            form: OrganisationForm::from_api(organisation),
            pending: false,
            error: None,
            confirm_delete: false,
            delete_pending: false,
        }
    }

    pub fn title(&self) -> String {
        if self.form.name.trim().is_empty() {
            if self.record_id.is_some() {
                "Organisation".to_owned()
            } else {
                "New organisation".to_owned()
            }
        } else {
            self.form.name.trim().to_owned()
        }
    }

    pub fn subtitle(&self) -> String {
        if self.record_id.is_some() {
            if self.form.is_active {
                "ORGANISATION · ACTIVE".to_owned()
            } else {
                "ORGANISATION · ARCHIVED".to_owned()
            }
        } else {
            "ORGANISATION · NEW".to_owned()
        }
    }

    pub fn apply_saved(&mut self, organisation: ApiOrganisation) {
        self.record_id = Some(organisation.id.clone());
        self.form = OrganisationForm::from_api(&organisation);
        self.pending = false;
        self.error = None;
        self.confirm_delete = false;
        self.delete_pending = false;
    }

    pub fn save_failed(&mut self, message: String) {
        self.pending = false;
        self.error = Some(message);
    }

    pub fn delete_failed(&mut self, message: String) {
        self.delete_pending = false;
        self.error = Some(message);
    }
}

impl Default for OrganisationEditorState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct OrganisationForm {
    /// `None` when creating, otherwise the record being edited.
    pub id: Option<String>,
    pub name: String,
    pub kind: String,
    pub abn: String,
    pub email: String,
    pub phone: String,
    pub website: String,
    pub notes: String,
    pub is_active: bool,
}

impl Default for OrganisationForm {
    fn default() -> Self {
        Self::new()
    }
}

impl OrganisationForm {
    pub fn new() -> Self {
        Self {
            id: None,
            name: String::new(),
            kind: String::new(),
            abn: String::new(),
            email: String::new(),
            phone: String::new(),
            website: String::new(),
            notes: String::new(),
            is_active: true,
        }
    }

    fn from_api(organisation: &ApiOrganisation) -> Self {
        Self {
            id: Some(organisation.id.clone()),
            name: organisation.name.clone(),
            kind: organisation.kind.clone().unwrap_or_default(),
            abn: organisation.abn.clone().unwrap_or_default(),
            email: organisation.email.clone().unwrap_or_default(),
            phone: organisation.phone.clone().unwrap_or_default(),
            website: organisation.website.clone().unwrap_or_default(),
            notes: organisation.notes.clone().unwrap_or_default(),
            is_active: organisation.is_active,
        }
    }

    pub(crate) fn write(&self) -> Result<ApiOrganisationWrite, String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("Give the organisation a name.".to_owned());
        }
        if name.chars().count() > 200 {
            return Err("The name must be 200 characters or fewer.".to_owned());
        }

        let abn = self
            .abn
            .chars()
            .filter(|character| !character.is_whitespace() && *character != '-')
            .collect::<String>();
        if !abn.is_empty()
            && (abn.chars().count() != 11 || !abn.chars().all(|c| c.is_ascii_digit()))
        {
            return Err("The ABN must be exactly 11 digits.".to_owned());
        }

        let email = optional(&self.email);
        if let Some(email) = &email
            && (!email.contains('@') || email.starts_with('@') || email.ends_with('@'))
        {
            return Err("The email address is not valid.".to_owned());
        }

        let website = optional(&self.website);
        if let Some(website) = &website
            && !(website.starts_with("http://") || website.starts_with("https://"))
        {
            return Err("The website must start with http:// or https://.".to_owned());
        }

        Ok(ApiOrganisationWrite {
            name: name.to_owned(),
            kind: optional(&self.kind),
            abn: (!abn.is_empty()).then_some(abn),
            email: optional(&self.email),
            phone: optional(&self.phone),
            website,
            notes: optional(&self.notes),
            is_active: self.is_active,
        })
    }
}

#[derive(Clone, Debug)]
pub struct OrganisationsState {
    pub search: String,
    pub search_due: Option<Instant>,
    pub status: OrganisationStatusFilter,
    pub items: Vec<ApiOrganisation>,
    pub total: usize,
    pub loading: bool,
    pub error: Option<String>,
    pub last_query_ms: Option<f32>,
    pub loaded_once: bool,
    /// True once a list request has been sent, including one that failed, so
    /// the page does not retry every frame while the server is unavailable.
    pub attempted: bool,
}

impl Default for OrganisationsState {
    fn default() -> Self {
        Self::new()
    }
}

impl OrganisationsState {
    pub fn new() -> Self {
        Self {
            search: String::new(),
            search_due: None,
            status: OrganisationStatusFilter::Active,
            items: Vec::new(),
            total: 0,
            loading: false,
            error: None,
            last_query_ms: None,
            loaded_once: false,
            attempted: false,
        }
    }

    pub fn reset(&mut self) {
        self.search.clear();
        self.status = OrganisationStatusFilter::Active;
        self.items.clear();
        self.total = 0;
        self.loading = false;
        self.error = None;
        self.loaded_once = false;
        self.attempted = false;
        self.search_due = None;
    }

    pub fn apply_list(&mut self, items: Vec<ApiOrganisation>, total: usize) {
        self.items = items;
        self.total = total;
        self.loading = false;
        self.error = None;
        self.loaded_once = true;
    }

    pub fn list_failed(&mut self, message: String) {
        self.loading = false;
        self.error = Some(message);
    }

    pub fn saved(&mut self, organisation: ApiOrganisation) {
        self.error = None;
        if let Some(index) = self
            .items
            .iter()
            .position(|existing| existing.id == organisation.id)
        {
            if self.matches_filters(&organisation) {
                self.items[index] = organisation;
            } else {
                self.items.remove(index);
                self.total = self.total.saturating_sub(1);
            }
        } else if self.matches_filters(&organisation) {
            self.items.push(organisation);
            self.total += 1;
        }
        self.sort_items();
    }

    pub fn deleted(&mut self, id: &str, archived: bool) {
        if archived {
            self.items.retain(|organisation| organisation.id != id);
            self.total = self.total.saturating_sub(1);
        } else if let Some(existing) = self
            .items
            .iter_mut()
            .find(|organisation| organisation.id == id)
        {
            existing.is_active = false;
        }
    }

    fn matches_filters(&self, organisation: &ApiOrganisation) -> bool {
        let status_matches = match self.status {
            OrganisationStatusFilter::Active => organisation.is_active,
            OrganisationStatusFilter::Inactive => !organisation.is_active,
            OrganisationStatusFilter::All => true,
        };
        if !status_matches {
            return false;
        }
        let needle = self.search.trim().to_lowercase();
        if needle.is_empty() {
            return true;
        }
        [
            Some(organisation.name.as_str()),
            organisation.kind.as_deref(),
            organisation.abn.as_deref(),
            organisation.email.as_deref(),
            organisation.phone.as_deref(),
            organisation.website.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|value| value.to_lowercase().contains(&needle))
    }

    fn sort_items(&mut self) {
        self.items.sort_by_key(|item| item.name.to_lowercase());
    }
}

pub fn show(
    ui: &mut Ui,
    state: &mut OrganisationsState,
    signed_in: bool,
) -> Option<OrganisationsAction> {
    if !signed_in {
        return sign_in_card(ui);
    }

    header(ui, state);
    let mut action = None;
    ui.add_space(12.0);
    action = search_row(ui, state, action);

    if let Some(error) = &state.error {
        ui.add_space(8.0);
        message_card(ui, "Could not load organisations", error, theme::danger());
    }

    ui.add_space(10.0);
    if state.loading && state.items.is_empty() {
        message_card(
            ui,
            "Loading organisations",
            "Fetching external organisations from the CareLibre server.",
            theme::muted(),
        );
    } else if state.items.is_empty() {
        let message = if state.search.trim().is_empty() {
            match state.status {
                OrganisationStatusFilter::Active => {
                    "No active organisations yet. Create the first one."
                }
                OrganisationStatusFilter::Inactive => "No archived organisations.",
                OrganisationStatusFilter::All => "No organisations yet. Create the first one.",
            }
        } else {
            "No organisations match this search."
        };
        message_card(ui, "No organisations", message, theme::muted());
    } else if action.is_none() {
        action = list(ui, state);
    }

    action
}

fn header(ui: &mut Ui, state: &OrganisationsState) {
    if ui.available_width() < 520.0 {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Organisations").font(theme::strong_font(26.0)));
                theme::pill(ui, &state.total.to_string(), theme::accent());
            });
            ui.label(
                RichText::new(
                    "Providers, brokers, health services and agencies your contacts work with.",
                )
                .color(theme::muted())
                .size(12.5),
            );
        });
    } else {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Organisations").font(theme::strong_font(26.0)));
            theme::pill(ui, &state.total.to_string(), theme::accent());
        });
        ui.label(
            RichText::new(
                "Providers, brokers, health services and agencies your contacts work with.",
            )
            .color(theme::muted())
            .size(12.5),
        );
    }
}

fn search_row(
    ui: &mut Ui,
    state: &mut OrganisationsState,
    mut action: Option<OrganisationsAction>,
) -> Option<OrganisationsAction> {
    ui.horizontal_wrapped(|ui| {
        let width = ui.available_width().min(360.0);
        let response = ui.add_sized(
            [width, 34.0],
            egui::TextEdit::singleline(&mut state.search)
                .hint_text("Search name, kind, ABN, email or phone…")
                .margin(Margin::symmetric(10, 8)),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Search organisations")
        });
        if response.changed() {
            state.search_due = Some(Instant::now() + SEARCH_DEBOUNCE);
            state.loading = true;
            action = Some(OrganisationsAction::SearchChanged);
        }

        for filter in OrganisationStatusFilter::ALL {
            if ui
                .selectable_label(state.status == filter, filter.label())
                .clicked()
                && state.status != filter
            {
                state.status = filter;
                state.loading = true;
                action = Some(OrganisationsAction::FilterChanged);
            }
        }

        if !state.search.is_empty() && ui.button("Clear").clicked() {
            state.search.clear();
            state.search_due = Some(Instant::now());
            state.loading = true;
            action = Some(OrganisationsAction::SearchChanged);
        }

        if ui.button("+ New organisation").clicked() {
            action = Some(OrganisationsAction::New);
        }
    });

    ui.horizontal(|ui| {
        let mut text = format!("{} shown · {} total", state.items.len(), state.total);
        if state.loading {
            text.push_str(" · updating");
        }
        ui.label(RichText::new(text).color(theme::faint()).size(10.5));
        if let Some(ms) = state.last_query_ms {
            ui.label(
                RichText::new(format!("{ms:.1} ms"))
                    .color(theme::faint())
                    .size(10.5)
                    .monospace(),
            );
        }
        if state.items.len() < state.total {
            ui.label(
                RichText::new("Refine the search to see the rest")
                    .color(theme::faint())
                    .size(10.5),
            );
        }
        if ui.button("Refresh").clicked() {
            action = Some(OrganisationsAction::Reload);
        }
    });

    action
}

/// The organisation editor rendered in its own column. The body scrolls while
/// the save/cancel footer stays pinned to the bottom.
pub fn show_editor(
    ui: &mut Ui,
    state: &mut OrganisationEditorState,
) -> Option<OrganisationEditorAction> {
    let mut action = None;
    let footer_height = if state.confirm_delete { 120.0 } else { 72.0 };
    // `available_height` can extend past the column's clip rect while the
    // outer layout is still settling, so size the body against the clipped
    // bottom. This keeps the save/cancel footer visible at any window height.
    let bottom = ui.clip_rect().bottom().min(ui.max_rect().bottom());
    let body_height = (bottom - ui.cursor().top() - footer_height).max(120.0);

    ScrollArea::vertical()
        .id_salt((
            "organisation-editor",
            state.record_id.as_deref().unwrap_or("new"),
        ))
        .max_height(body_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(if state.record_id.is_some() {
                    "Edit organisation"
                } else {
                    "New organisation"
                })
                .font(theme::strong_font(15.0))
                .color(theme::text()),
            );
            ui.label(
                RichText::new(
                    "Counterparties such as providers, brokers, health services and agencies.",
                )
                .color(theme::muted())
                .size(10.5),
            );
            ui.add_space(8.0);

            field(ui, "Name", &mut state.form.name);
            ui.columns(2, |columns| {
                kind_field(&mut columns[0], "Kind", &mut state.form.kind);
                field(&mut columns[1], "ABN", &mut state.form.abn);
            });
            ui.columns(2, |columns| {
                field(&mut columns[0], "Email", &mut state.form.email);
                field(&mut columns[1], "Phone", &mut state.form.phone);
            });
            field(ui, "Website", &mut state.form.website);

            ui.label(RichText::new("Notes").color(theme::faint()).size(10.5));
            let notes = ui.add(
                egui::TextEdit::multiline(&mut state.form.notes)
                    .desired_rows(3)
                    .desired_width(ui.available_width())
                    .margin(Margin::symmetric(8, 6)),
            );
            notes.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Notes"));

            ui.checkbox(&mut state.form.is_active, "Active");
            ui.label(
                RichText::new(
                    "Archived organisations stay in relationship history and can be restored by ticking Active.",
                )
                .color(theme::faint())
                .size(10.0),
            );

            if let Some(error) = &state.error {
                ui.add_space(6.0);
                ui.label(RichText::new(error).color(theme::danger()).size(11.0));
            }
        });

    ui.add_space(4.0);
    ui.separator();
    ui.add_space(6.0);

    if state.confirm_delete {
        ui.label(
            RichText::new("Delete this organisation?")
                .font(theme::strong_font(11.5))
                .color(theme::text()),
        );
        ui.label(
            RichText::new("It will be archived and hidden from the active list.")
                .color(theme::muted())
                .size(10.0),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !state.delete_pending,
                    egui::Button::new(
                        RichText::new("Delete organisation")
                            .color(theme::danger())
                            .size(11.5),
                    ),
                )
                .clicked()
            {
                state.delete_pending = true;
                state.error = None;
                action = Some(OrganisationEditorAction::Delete);
            }
            if ui
                .add_enabled(!state.delete_pending, theme::secondary_button("Cancel"))
                .clicked()
            {
                state.confirm_delete = false;
            }
            if state.delete_pending {
                ui.label(RichText::new("Deleting…").color(theme::muted()).size(10.5));
            }
        });
    } else {
        ui.horizontal(|ui| {
            let save_label = if state.record_id.is_some() {
                "Save changes"
            } else {
                "Create organisation"
            };
            if ui
                .add_enabled(!state.pending, theme::primary_button(save_label))
                .clicked()
            {
                match state.form.write() {
                    Ok(_) => {
                        state.pending = true;
                        state.error = None;
                        action = Some(OrganisationEditorAction::Save);
                    }
                    Err(error) => state.error = Some(error),
                }
            }
            if ui
                .add_enabled(!state.pending, theme::secondary_button("Cancel"))
                .clicked()
            {
                action = Some(OrganisationEditorAction::Cancel);
            }
            if state.record_id.is_some() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            !state.pending,
                            egui::Button::new(
                                RichText::new("Delete").color(theme::danger()).size(11.0),
                            )
                            .frame(false),
                        )
                        .clicked()
                    {
                        state.confirm_delete = true;
                        state.error = None;
                    }
                });
            }
            if state.pending {
                ui.label(RichText::new("Saving…").color(theme::muted()).size(10.5));
            }
        });
    }

    action
}

fn list(ui: &mut Ui, state: &mut OrganisationsState) -> Option<OrganisationsAction> {
    let mut edit: Option<String> = None;

    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            for organisation in &state.items {
                if row(ui, organisation) {
                    edit = Some(organisation.id.clone());
                }
            }
        });

    edit.map(OrganisationsAction::Edit)
}

fn row(ui: &mut Ui, organisation: &ApiOrganisation) -> bool {
    let has_notes = organisation
        .notes
        .as_deref()
        .is_some_and(|notes| !notes.trim().is_empty());
    let height = if has_notes { 112.0 } else { 96.0 };
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
    let card = rect.shrink2(egui::vec2(0.0, 4.0));
    let hovered = ui
        .ctx()
        .pointer_hover_pos()
        .is_some_and(|position| card.contains(position));

    let fill = if hovered {
        theme::surface()
    } else {
        theme::surface_raised()
    };
    ui.painter().rect(
        card,
        CornerRadius::same(10),
        fill,
        Stroke::new(1.0, theme::border()),
        StrokeKind::Inside,
    );

    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(card.shrink2(egui::vec2(12.0, 8.0)))
            .layout(Layout::top_down(Align::Min)),
    );
    content.set_width((card.width() - 24.0).max(1.0));
    content.set_clip_rect(card.intersect(ui.clip_rect()));

    content.horizontal(|ui| {
        ui.add(
            egui::Label::new(
                RichText::new(&organisation.name)
                    .font(theme::strong_font(13.5))
                    .color(theme::text()),
            )
            .truncate(),
        );
        if let Some(kind) = &organisation.kind {
            theme::pill(ui, &kind_label(kind), theme::accent_blue());
        }
        if !organisation.is_active {
            theme::pill(ui, "Archived", theme::warning());
        }
    });

    let mut details = Vec::new();
    if let Some(abn) = &organisation.abn {
        details.push(format!("ABN {abn}"));
    }
    if let Some(email) = &organisation.email {
        details.push(email.clone());
    }
    if let Some(phone) = &organisation.phone {
        details.push(phone.clone());
    }
    if let Some(website) = &organisation.website {
        details.push(website.clone());
    }
    if !details.is_empty() {
        content.add(
            egui::Label::new(
                RichText::new(details.join("  ·  "))
                    .color(theme::muted())
                    .size(10.0),
            )
            .truncate(),
        );
    }

    let relationships = organisation.relationship_count;
    content.label(
        RichText::new(format!(
            "{relationships} relationship{}",
            if relationships == 1 { "" } else { "s" }
        ))
        .color(theme::faint())
        .size(10.0),
    );
    if let Some(notes) = &organisation.notes
        && !notes.trim().is_empty()
    {
        content.add(
            egui::Label::new(
                RichText::new(notes)
                    .color(theme::faint())
                    .size(10.0)
                    .italics(),
            )
            .truncate(),
        );
    }

    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    // Register the card last so it sits above the label widgets and receives
    // the click no matter which part of the card was pressed.
    let response = ui.interact(
        card,
        ui.scope_id().with(("organisation-card", &organisation.id)),
        Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!("Edit {}", organisation.name),
        )
    });
    response.clicked()
}

fn kind_field(ui: &mut Ui, label: &str, value: &mut String) {
    ui.vertical(|ui| {
        ui.label(RichText::new(label).color(theme::faint()).size(10.5));
        let mut current = value.clone();
        let response = egui::ComboBox::from_id_salt("organisation-kind")
            .selected_text(if current.is_empty() {
                "Select…".to_owned()
            } else {
                kind_label(&current)
            })
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut current, String::new(), "Not set");
                for kind in ORGANISATION_KINDS {
                    ui.selectable_value(&mut current, (*kind).to_owned(), kind_label(kind));
                }
                // Records written before Kind became a picklist may hold an
                // arbitrary descriptor. Keep it selectable so saving an
                // unrelated change does not silently clear it.
                if !current.is_empty() && !ORGANISATION_KINDS.contains(&current.as_str()) {
                    let legacy = current.clone();
                    ui.selectable_value(&mut current, legacy.clone(), kind_label(&legacy));
                }
            });
        response.response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::ComboBox, ui.is_enabled(), label)
        });
        if current != *value {
            *value = current;
        }
    });
}

fn field(ui: &mut Ui, label: &str, value: &mut String) {
    ui.vertical(|ui| {
        ui.label(RichText::new(label).color(theme::faint()).size(10.5));
        let response = ui.add(
            egui::TextEdit::singleline(value)
                .desired_width(ui.available_width())
                .margin(Margin::symmetric(8, 6)),
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
    });
}

fn message_card(ui: &mut Ui, title: &str, message: &str, colour: egui::Color32) {
    Frame::new()
        .fill(theme::surface())
        .stroke(Stroke::new(1.0, theme::border()))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(title)
                    .font(theme::strong_font(14.0))
                    .color(colour),
            );
            ui.label(RichText::new(message).color(theme::muted()).size(11.0));
        });
}

fn sign_in_card(ui: &mut Ui) -> Option<OrganisationsAction> {
    Frame::new()
        .fill(theme::surface())
        .stroke(Stroke::new(1.0, theme::border()))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("Sign in to manage organisations")
                    .font(theme::strong_font(15.0))
                    .color(theme::text()),
            );
            ui.label(
                RichText::new(
                    "Organisation records live on the CareLibre server. Sign in from Settings to list, create, edit and delete them.",
                )
                .color(theme::muted())
                .size(11.0),
            );
        });
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn organisation(name: &str, active: bool) -> ApiOrganisation {
        ApiOrganisation {
            id: name.to_lowercase(),
            name: name.to_owned(),
            kind: Some("provider".to_owned()),
            abn: None,
            email: None,
            phone: None,
            website: None,
            notes: None,
            is_active: active,
            relationship_count: 0,
            updated_at: String::new(),
        }
    }

    #[test]
    fn form_accepts_and_normalises_a_valid_record() {
        let mut form = OrganisationForm::new();
        form.name = "  Northside Care  ".to_owned();
        form.abn = "12 345 678 901".to_owned();
        form.website = "https://northside.example".to_owned();
        let write = form.write().expect("valid form");
        assert_eq!(write.name, "Northside Care");
        assert_eq!(write.abn.as_deref(), Some("12345678901"));
    }

    #[test]
    fn form_rejects_a_bad_abn_and_website() {
        let mut form = OrganisationForm::new();
        form.name = "Care Co".to_owned();
        form.abn = "123".to_owned();
        assert!(form.write().is_err());

        form.abn.clear();
        form.website = "care.example".to_owned();
        assert!(form.write().is_err());
    }

    #[test]
    fn saved_records_respect_the_active_filter() {
        let mut state = OrganisationsState::new();
        state.status = OrganisationStatusFilter::Active;
        state.saved(organisation("Northside Care", true));
        state.saved(organisation("Archived Care", false));
        assert_eq!(state.items.len(), 1);
        assert_eq!(state.items[0].name, "Northside Care");
    }

    #[test]
    fn deleting_removes_the_active_row() {
        let mut state = OrganisationsState::new();
        state.apply_list(vec![organisation("Northside Care", true)], 1);
        state.deleted("northside care", true);
        assert!(state.items.is_empty());
        assert_eq!(state.total, 0);
    }

    #[test]
    fn editor_delete_needs_confirmation() {
        let mut editor = OrganisationEditorState::edit(&organisation("Northside Care", true));
        assert!(!editor.confirm_delete);
        editor.confirm_delete = true;
        assert!(editor.confirm_delete);
    }
}
