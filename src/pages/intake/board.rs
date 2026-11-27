//! Server-backed intake kanban board and case detail.

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui::{self, Align, Layout, RichText, ScrollArea, Stroke, Ui};
use serde_json::{Value, json};

use crate::{
    intake_board::{
        ApiContactMatch, ApiIntakeBoard, ApiIntakeCard, ApiIntakeCaseDetail, ApiIntakeField,
        ApiIntakeFlow,
    },
    theme,
};

pub struct IntakeBoardState {
    pub flows: Vec<ApiIntakeFlow>,
    pub flows_loading: bool,
    pub flows_requested: bool,
    pub flows_error: Option<String>,
    pub flows_request_id: Option<u64>,
    pub fields: Vec<ApiIntakeField>,
    pub fields_loading: bool,
    pub fields_requested: bool,
    pub fields_request_id: Option<u64>,
    pub fields_error: Option<String>,
    /// Empty means every active flow is included.
    pub selected_flows: BTreeSet<String>,
    pub subject_kind: Option<String>,
    pub assignee_filter: String,
    pub board: Option<ApiIntakeBoard>,
    pub board_loading: bool,
    pub board_requested: bool,
    pub board_error: Option<String>,
    pub board_request_id: Option<u64>,
    pub search: String,
    pub request_seq: u64,
    pub new_open: bool,
    pub new_flow_id: Option<String>,
    pub new_mode: String,
    pub new_search: String,
    pub new_candidates: Vec<ApiContactMatch>,
    pub new_contact_id: Option<String>,
    pub new_given_name: String,
    pub new_family_name: String,
    pub new_email: String,
    pub new_phone: String,
    pub new_date_of_birth: String,
    pub new_error: Option<String>,
    pub new_saving: bool,
    pub new_request_id: Option<u64>,
    pub contacts_request_id: Option<u64>,
}

impl Default for IntakeBoardState {
    fn default() -> Self {
        Self {
            flows: Vec::new(),
            flows_loading: false,
            flows_requested: false,
            flows_error: None,
            flows_request_id: None,
            fields: Vec::new(),
            fields_loading: false,
            fields_requested: false,
            fields_request_id: None,
            fields_error: None,
            selected_flows: BTreeSet::new(),
            subject_kind: None,
            assignee_filter: "all".to_owned(),
            board: None,
            board_loading: false,
            board_requested: false,
            board_error: None,
            board_request_id: None,
            search: String::new(),
            request_seq: 0,
            new_open: false,
            new_flow_id: None,
            new_mode: "new".to_owned(),
            new_search: String::new(),
            new_candidates: Vec::new(),
            new_contact_id: None,
            new_given_name: String::new(),
            new_family_name: String::new(),
            new_email: String::new(),
            new_phone: String::new(),
            new_date_of_birth: String::new(),
            new_error: None,
            new_saving: false,
            new_request_id: None,
            contacts_request_id: None,
        }
    }
}

impl IntakeBoardState {
    pub fn next_request(&mut self) -> u64 {
        self.request_seq += 1;
        self.request_seq
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn reset_board(&mut self) {
        self.board = None;
        self.board_loading = false;
        self.board_requested = false;
        self.board_error = None;
        self.board_request_id = None;
    }

    pub fn flow_filter_label(&self) -> String {
        if self.selected_flows.is_empty() {
            "All intake paths".to_owned()
        } else {
            format!("{} path(s)", self.selected_flows.len())
        }
    }

    pub fn flow_filter_ids(&self) -> Vec<String> {
        self.selected_flows.iter().cloned().collect()
    }
}

pub struct IntakeCaseColumnState {
    pub case_id: String,
    pub case: Option<ApiIntakeCaseDetail>,
    pub loading: bool,
    pub requested: bool,
    pub request_id: Option<u64>,
    pub error: Option<String>,
    pub transitioning: bool,
    pub transition_request_id: Option<u64>,
    pub transition_error: Option<String>,
    pub drafts: BTreeMap<String, String>,
    pub booleans: BTreeMap<String, bool>,
    pub dirty: bool,
    pub saving: bool,
    pub save_request_id: Option<u64>,
    pub save_error: Option<String>,
    pub cancel_open: bool,
    pub cancel_reason: String,
    pub cancel_saving: bool,
    pub cancel_request_id: Option<u64>,
    pub cancel_error: Option<String>,
    pub checklist_pending: bool,
    pub checklist_request_id: Option<u64>,
    pub checklist_error: Option<String>,
}

impl IntakeCaseColumnState {
    pub fn new(case_id: String) -> Self {
        Self {
            case_id,
            case: None,
            loading: false,
            requested: false,
            request_id: None,
            error: None,
            transitioning: false,
            transition_request_id: None,
            transition_error: None,
            drafts: BTreeMap::new(),
            booleans: BTreeMap::new(),
            dirty: false,
            saving: false,
            save_request_id: None,
            save_error: None,
            cancel_open: false,
            cancel_reason: String::new(),
            cancel_saving: false,
            cancel_request_id: None,
            cancel_error: None,
            checklist_pending: false,
            checklist_request_id: None,
            checklist_error: None,
        }
    }

    pub fn apply_case(&mut self, case: ApiIntakeCaseDetail, fields: &[ApiIntakeField]) {
        self.case = Some(case);
        self.error = None;
        self.loading = false;
        self.requested = false;
        self.request_id = None;
        self.transitioning = false;
        self.transition_request_id = None;
        self.transition_error = None;
        self.save_error = None;
        self.dirty = false;
        self.cancel_open = false;
        self.cancel_saving = false;
        self.cancel_request_id = None;
        self.cancel_error = None;
        self.checklist_pending = false;
        self.checklist_request_id = None;
        self.checklist_error = None;
        self.load_drafts(fields);
    }

    pub fn load_drafts(&mut self, fields: &[ApiIntakeField]) {
        self.drafts.clear();
        self.booleans.clear();
        let Some(case) = &self.case else {
            return;
        };
        for key in &case.visible_fields {
            let Some(field) = fields.iter().find(|field| &field.key == key) else {
                continue;
            };
            let value = case.case.data.get(key).cloned().unwrap_or(Value::Null);
            match field.data_type.as_str() {
                "boolean" => {
                    self.booleans
                        .insert(key.clone(), value.as_bool().unwrap_or(false));
                }
                "multiselect" => {
                    let text = value
                        .as_array()
                        .map(|values| {
                            values
                                .iter()
                                .filter_map(|value| value.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    self.drafts.insert(key.clone(), text);
                }
                _ => {
                    let text = match value {
                        Value::Null => String::new(),
                        Value::String(value) => value,
                        other => other.to_string(),
                    };
                    self.drafts.insert(key.clone(), text);
                }
            }
        }
    }

    pub fn build_patch(&self, fields: &[ApiIntakeField]) -> Value {
        let Some(case) = &self.case else {
            return json!({});
        };
        let mut data = serde_json::Map::new();
        for key in &case.visible_fields {
            let Some(field) = fields.iter().find(|field| &field.key == key) else {
                continue;
            };
            let value = match field.data_type.as_str() {
                "boolean" => Value::Bool(self.booleans.get(key).copied().unwrap_or(false)),
                "integer" => self
                    .drafts
                    .get(key)
                    .map(|value| value.trim())
                    .filter(|value| !value.is_empty())
                    .and_then(|value| value.parse::<i64>().ok())
                    .map(|value| json!(value))
                    .unwrap_or(Value::Null),
                "decimal" => self
                    .drafts
                    .get(key)
                    .map(|value| value.trim())
                    .filter(|value| !value.is_empty())
                    .and_then(|value| value.parse::<f64>().ok())
                    .map(|value| json!(value))
                    .unwrap_or(Value::Null),
                "multiselect" => {
                    let values = self
                        .drafts
                        .get(key)
                        .map(|value| {
                            value
                                .split(',')
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(|value| Value::String(value.to_owned()))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    if values.is_empty() {
                        Value::Null
                    } else {
                        Value::Array(values)
                    }
                }
                _ => {
                    let text = self.drafts.get(key).map(String::as_str).unwrap_or("");
                    if text.trim().is_empty() {
                        Value::Null
                    } else {
                        Value::String(text.trim().to_owned())
                    }
                }
            };
            data.insert(key.clone(), value);
        }
        Value::Object(data)
    }

    pub fn title(&self) -> String {
        self.case
            .as_ref()
            .map(|case| case.case.display_name.clone())
            .unwrap_or_else(|| "Intake case".to_owned())
    }

    pub fn subtitle(&self) -> String {
        self.case
            .as_ref()
            .map(|case| format!("{} · {}", case.case.stage.name, case.case.flow.name))
            .unwrap_or_else(|| "INTAKE CASE".to_owned())
    }
}

pub enum IntakeBoardAction {
    ReloadFlows,
    ReloadBoard,
    Search,
    OpenCase(String),
    OpenNewIntake,
    CloseNewIntake,
    SearchNewContacts,
    CreateIntake,
}

pub enum IntakeCaseAction {
    Reload,
    SaveFields,
    ToggleChecklist {
        item_id: String,
        completed: bool,
    },
    Transition {
        case_id: String,
        stage_id: String,
        version: i32,
    },
    Cancel {
        reason: String,
    },
}

pub fn show(
    ui: &mut Ui,
    state: &mut IntakeBoardState,
    signed_in: bool,
) -> Option<IntakeBoardAction> {
    let mut action = None;
    if !signed_in {
        header(
            ui,
            "Intake",
            "Sign in to the server to use the shared intake board.",
        );
        ui.add_space(16.0);
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("Server sign-in required")
                    .font(theme::strong_font(16.0)),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Intake cases, referrals and waitlists are shared through the CareLibre server.",
                )
                .color(theme::muted())
                .size(12.0),
            );
        });
        return None;
    }

    header(
        ui,
        "Intake",
        "Enquiries, referrals and onboarding across configurable stages.",
    );
    ui.add_space(12.0);

    toolbar(ui, state, &mut action);
    ui.add_space(12.0);

    if let Some(error) = &state.fields_error {
        ui.colored_label(
            theme::warning(),
            format!("The intake field editor could not load: {error}"),
        );
        ui.add_space(8.0);
    }

    if state.new_open {
        new_intake_dialog(ui.ctx(), state, &mut action);
    }

    if state.flows_loading && state.flows.is_empty() {
        loading(ui, "Loading intake flows…");
        return action;
    }
    if let Some(error) = &state.flows_error {
        error_card(ui, "Could not load intake flows", error);
        if ui.button("Retry").clicked() {
            action = Some(IntakeBoardAction::ReloadFlows);
        }
        return action;
    }
    if state.flows.is_empty() {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("No intake flows configured").font(theme::strong_font(15.0)));
            ui.label(
                RichText::new("An administrator can create flows in the server admin panel.")
                    .color(theme::muted())
                    .size(12.0),
            );
        });
        return action;
    }

    if state.board_loading && state.board.is_none() {
        loading(ui, "Loading the board…");
        return action;
    }
    if let Some(error) = &state.board_error {
        error_card(ui, "Could not load the board", error);
        if ui.button("Retry").clicked() {
            action = Some(IntakeBoardAction::ReloadBoard);
        }
        return action;
    }

    if let Some(board) = state.board.clone() {
        if board.columns.iter().all(|column| column.count == 0) {
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("No cases on this board").font(theme::strong_font(15.0)));
                ui.label(
                    RichText::new(
                        "New enquiries and referrals will appear here as they are created.",
                    )
                    .color(theme::muted())
                    .size(12.0),
                );
            });
        } else {
            board_columns(ui, &board, &mut action);
        }
    }

    action
}

fn intake_flow_card(ui: &mut Ui, flow: &ApiIntakeFlow, selected: bool) -> bool {
    let mut text = egui::text::LayoutJob::default();
    text.append(
        &flow.name,
        0.0,
        egui::text::TextFormat {
            font_id: theme::strong_font(13.0),
            color: theme::text(),
            ..Default::default()
        },
    );
    text.append("\n", 0.0, egui::text::TextFormat::default());
    let description = flow
        .description
        .as_deref()
        .map(str::trim)
        .filter(|description| !description.is_empty());
    match description {
        Some(description) => {
            text.append(
                description,
                0.0,
                egui::text::TextFormat {
                    font_id: egui::FontId::proportional(11.0),
                    color: theme::muted(),
                    ..Default::default()
                },
            );
        }
        None => {
            text.append(
                "No description set for this path.",
                0.0,
                egui::text::TextFormat {
                    font_id: egui::FontId::proportional(11.0),
                    color: theme::faint(),
                    ..Default::default()
                },
            );
        }
    }
    let response = ui.add(
        egui::Button::new(text)
            .selected(selected)
            .frame(true)
            .wrap()
            .fill(if selected {
                theme::surface_raised()
            } else {
                theme::surface()
            })
            .stroke(if selected {
                Stroke::new(1.5, theme::accent())
            } else {
                Stroke::new(1.0, theme::border())
            })
            .corner_radius(egui::CornerRadius::same(10))
            .min_size(egui::vec2(ui.available_width(), 0.0)),
    );
    response.clicked()
}

fn new_intake_dialog(
    ctx: &egui::Context,
    state: &mut IntakeBoardState,
    action: &mut Option<IntakeBoardAction>,
) {
    let response = egui::Modal::new(egui::Id::unique("new-intake-modal"))
        .backdrop_color(egui::Color32::from_black_alpha(170))
        .frame(theme::card())
        .show(ctx, |ui| {
            ui.set_width(600.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("New intake").font(theme::strong_font(19.0)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new(RichText::new("Close").size(11.0)).frame(false))
                        .clicked()
                    {
                        *action = Some(IntakeBoardAction::CloseNewIntake);
                    }
                });
            });
            ui.label(
                RichText::new(
                    "Choose the intake path. Contact details are added once the record is open.",
                )
                .color(theme::muted())
                .size(12.0),
            );
            ui.add_space(10.0);
            let selected_id = state.new_flow_id.clone();
            ScrollArea::vertical()
                .id_salt("new-intake-paths")
                .max_height(300.0)
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for flow in &state.flows {
                        let selected = selected_id.as_deref() == Some(flow.id.as_str());
                        if intake_flow_card(ui, flow, selected) {
                            state.new_flow_id = Some(flow.id.clone());
                        }
                        ui.add_space(4.0);
                    }
                });

            if let Some(error) = &state.new_error {
                ui.add_space(6.0);
                ui.colored_label(theme::danger(), error);
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!state.new_saving, |ui| {
                    if ui.add(theme::primary_button("Create intake")).clicked() {
                        *action = Some(IntakeBoardAction::CreateIntake);
                    }
                });
                if ui.button("Cancel").clicked() {
                    *action = Some(IntakeBoardAction::CloseNewIntake);
                }
                if state.new_saving {
                    ui.spinner();
                }
            });
        });
    if response.should_close() {
        *action = Some(IntakeBoardAction::CloseNewIntake);
    }
}

fn header(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).font(theme::strong_font(24.0)));
    ui.label(RichText::new(subtitle).color(theme::muted()).size(12.0));
}

fn toolbar(ui: &mut Ui, state: &mut IntakeBoardState, action: &mut Option<IntakeBoardAction>) {
    ui.horizontal_wrapped(|ui| {
        let label = state.flow_filter_label();
        let mut flow_changed = false;
        ui.menu_button(label, |ui| {
            ui.set_min_width(260.0);
            if ui.button("All intake paths").clicked() {
                state.selected_flows.clear();
                flow_changed = true;
                ui.close();
            }
            ui.separator();
            for flow in &state.flows {
                let mut selected = state.selected_flows.contains(&flow.id);
                if ui.checkbox(&mut selected, flow.name.clone()).changed() {
                    if selected {
                        state.selected_flows.insert(flow.id.clone());
                    } else {
                        state.selected_flows.remove(&flow.id);
                    }
                    flow_changed = true;
                }
            }
        });
        if flow_changed {
            *action = Some(IntakeBoardAction::ReloadBoard);
        }

        let previous_type = state.subject_kind.clone();
        ui.menu_button(subject_kind_label(state.subject_kind.as_deref()), |ui| {
            ui.set_min_width(180.0);
            if ui
                .selectable_label(state.subject_kind.is_none(), "All types")
                .clicked()
            {
                state.subject_kind = None;
                ui.close();
            }
            for (value, label) in [
                ("client", "Clients"),
                ("worker", "Workers"),
                ("other_contact", "Other contacts"),
                ("organisation", "Organisations"),
            ] {
                let selected = state.subject_kind.as_deref() == Some(value);
                if ui.selectable_label(selected, label).clicked() {
                    state.subject_kind = Some(value.to_owned());
                    ui.close();
                }
            }
        });
        if previous_type != state.subject_kind {
            *action = Some(IntakeBoardAction::ReloadBoard);
        }

        let previous_assignee = state.assignee_filter.clone();
        ui.menu_button(assignee_label(&state.assignee_filter), |ui| {
            ui.set_min_width(160.0);
            for (value, label) in [
                ("all", "Everyone"),
                ("me", "My cases"),
                ("unassigned", "Unassigned"),
            ] {
                let selected = state.assignee_filter == value;
                if ui.selectable_label(selected, label).clicked() {
                    state.assignee_filter = value.to_owned();
                    ui.close();
                }
            }
        });
        if previous_assignee != state.assignee_filter {
            *action = Some(IntakeBoardAction::ReloadBoard);
        }

        let response = ui.add_sized(
            [240.0, 32.0],
            egui::TextEdit::singleline(&mut state.search)
                .hint_text("Search cases, people, email or phone"),
        );
        if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            *action = Some(IntakeBoardAction::Search);
        }
        if ui.button("Search").clicked() {
            *action = Some(IntakeBoardAction::Search);
        }
        if ui.button("Refresh").clicked() {
            *action = Some(IntakeBoardAction::ReloadBoard);
        }
        if ui.add(theme::primary_button("+ New intake")).clicked() {
            *action = Some(IntakeBoardAction::OpenNewIntake);
        }
    });
}

fn board_columns(ui: &mut Ui, board: &ApiIntakeBoard, action: &mut Option<IntakeBoardAction>) {
    let multi_flow = board.flows.len() > 1;
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let available = (ui.available_width() - BOARD_RIGHT_PADDING).max(1.0);
            let (per_row, width) = board_layout(available);
            for row in board.columns.chunks(per_row) {
                ui.horizontal_top(|ui| {
                    for (position, column) in row.iter().enumerate() {
                        if position > 0 {
                            ui.add_space(BOARD_GAP);
                        }
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            board_column(ui, column, multi_flow, action);
                        });
                    }
                });
                ui.add_space(BOARD_GAP);
            }
        });
}

const BOARD_MIN_COLUMN_WIDTH: f32 = 240.0;
const BOARD_GAP: f32 = 8.0;
const BOARD_RIGHT_PADDING: f32 = 12.0;

/// Fit as many columns as possible on each row without horizontal scrolling.
/// A single column always takes the full available width, however narrow.
fn board_layout(available: f32) -> (usize, f32) {
    let available = available.max(1.0);
    let per_row =
        (((available + BOARD_GAP) / (BOARD_MIN_COLUMN_WIDTH + BOARD_GAP)).floor() as usize).max(1);
    let width = if per_row == 1 {
        available
    } else {
        ((available - BOARD_GAP * (per_row.saturating_sub(1)) as f32) / per_row as f32)
            .max(BOARD_MIN_COLUMN_WIDTH)
    };
    (per_row, width)
}

fn board_column(
    ui: &mut Ui,
    column: &crate::intake_board::ApiIntakeColumn,
    multi_flow: bool,
    action: &mut Option<IntakeBoardAction>,
) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(&column.stage.name).font(theme::strong_font(13.0)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                theme::pill(ui, &column.count.to_string(), theme::accent());
            });
        });
        ui.add_space(4.0);
        if column.cards.is_empty() {
            ui.label(RichText::new("No cases").color(theme::faint()).size(11.0));
        }
        for card in &column.cards {
            if card_button(ui, card, multi_flow) {
                *action = Some(IntakeBoardAction::OpenCase(card.id.clone()));
            }
        }
    });
}

fn card_button(ui: &mut Ui, card: &ApiIntakeCard, show_flow: bool) -> bool {
    let mut meta = Vec::new();
    if show_flow {
        meta.push(card.flow.name.clone());
    }
    if let Some(lifecycle) = &card.lifecycle_status {
        meta.push(lifecycle.clone());
    }
    if let Some(division) = &card.division {
        meta.push(division.name.clone());
    }
    meta.push(
        card.assignee
            .as_ref()
            .map(|user| user.full_name.clone())
            .unwrap_or_else(|| "unassigned".to_owned()),
    );
    if card.waitlisted {
        meta.push("waitlist".to_owned());
    }
    let due = card
        .stage_due_at
        .as_deref()
        .map(|due| format!(" · due {}", short_date(due)))
        .unwrap_or_default();
    let text = format!(
        "{}\n{}{}\nchecklist {}/{}",
        card.display_name,
        meta.join(" · "),
        due,
        card.checklist.done,
        card.checklist.total
    );
    ui.add_sized(
        [ui.available_width(), 82.0],
        egui::Button::new(RichText::new(text).size(12.0))
            .wrap()
            .fill(theme::surface_raised())
            .stroke(Stroke::new(1.0, theme::border())),
    )
    .clicked()
}

pub fn show_case_column(
    ui: &mut Ui,
    state: &mut IntakeCaseColumnState,
    fields: &[ApiIntakeField],
) -> Option<IntakeCaseAction> {
    let mut action = None;
    if state.loading && state.case.is_none() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(
                RichText::new("Loading intake case…")
                    .color(theme::muted())
                    .size(12.0),
            );
        });
        return None;
    }
    if let Some(error) = &state.error {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Could not load the case").font(theme::strong_font(14.0)));
            ui.label(RichText::new(error).color(theme::danger()).size(12.0));
        });
        if ui.button("Retry").clicked() {
            return Some(IntakeCaseAction::Reload);
        }
        return None;
    }
    let case = state.case.clone()?;
    case_body(ui, state, &case, fields, &mut action);
    action
}

fn case_body(
    ui: &mut Ui,
    state: &mut IntakeCaseColumnState,
    case: &ApiIntakeCaseDetail,
    fields: &[ApiIntakeField],
    action: &mut Option<IntakeCaseAction>,
) {
    ScrollArea::vertical().show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            theme::pill(ui, &case.case.stage.name, theme::accent());
            if let Some(lifecycle) = &case.case.lifecycle_status {
                theme::pill(ui, lifecycle, theme::warning());
            }
            if case.case.waitlisted {
                theme::pill(ui, "Waitlisted", theme::warning());
            }
        });
        if let Some(contact) = &case.contact {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("{} · {}", contact.display_name, contact.role))
                    .color(theme::muted())
                    .size(12.0),
            );
        }
        ui.label(
            RichText::new(format!("Intake path: {}", case.case.flow.name))
                .color(theme::muted())
                .size(12.0),
        );
        if let Some(assignee) = &case.case.assignee {
            ui.label(
                RichText::new(format!("Assigned to {}", assignee.full_name))
                    .color(theme::muted())
                    .size(12.0),
            );
        }
        if let Some(division) = &case.case.division {
            ui.label(
                RichText::new(format!("Division: {}", division.name))
                    .color(theme::muted())
                    .size(12.0),
            );
        }

        ui.add_space(10.0);
        ui.label(RichText::new("Case fields").font(theme::strong_font(14.0)));
        for key in &case.visible_fields {
            let Some(field) = fields.iter().find(|field| &field.key == key) else {
                continue;
            };
            field_editor(ui, state, field, case.required_fields.contains(key));
        }
        if state.dirty {
            ui.add_space(6.0);
            ui.add_enabled_ui(!state.saving, |ui| {
                if ui.add(theme::primary_button("Save fields")).clicked() {
                    *action = Some(IntakeCaseAction::SaveFields);
                }
            });
            if state.saving {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Saving…").color(theme::muted()).size(11.0));
                });
            }
        }
        if let Some(error) = &state.save_error {
            ui.colored_label(theme::danger(), error);
        }

        let captured = case
            .case
            .data
            .as_object()
            .map(|data| {
                data.iter()
                    .filter(|(key, _)| !case.visible_fields.contains(key))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !captured.is_empty() {
            ui.add_space(10.0);
            ui.label(RichText::new("Captured so far").font(theme::strong_font(13.0)));
            for (key, value) in captured {
                if value.is_null() {
                    continue;
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(humanize_key(key))
                            .color(theme::muted())
                            .size(11.0),
                    );
                    ui.label(RichText::new(format_value(value)).size(12.0));
                });
            }
        }

        if !case.required_fields.is_empty() {
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!(
                    "This stage requires: {}",
                    case.required_fields
                        .iter()
                        .map(|key| humanize_key(key))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
                .color(theme::muted())
                .size(11.0),
            );
        }

        ui.add_space(12.0);
        ui.label(RichText::new("Checklist").font(theme::strong_font(14.0)));
        if case.checklist.is_empty() {
            ui.label(
                RichText::new("No checklist items for this stage.")
                    .color(theme::faint())
                    .size(11.0),
            );
        }
        for item in &case.checklist {
            let mut completed = item.completed_at.is_some();
            ui.add_enabled_ui(!state.checklist_pending, |ui| {
                if ui
                    .checkbox(
                        &mut completed,
                        format!(
                            "{}{}",
                            item.label,
                            if item.is_required { " · required" } else { "" }
                        ),
                    )
                    .changed()
                {
                    *action = Some(IntakeCaseAction::ToggleChecklist {
                        item_id: item.id.clone(),
                        completed,
                    });
                }
            });
            if let Some(description) = &item.description {
                ui.label(RichText::new(description).color(theme::faint()).size(11.0));
            }
        }
        if let Some(error) = &state.checklist_error {
            ui.colored_label(theme::danger(), error);
        }

        ui.add_space(12.0);
        ui.label(RichText::new("Path").font(theme::strong_font(14.0)));
        let sequential = case.flow.stage_movement == "sequential";
        let enabled = !state.transitioning && !state.dirty && !case.stage.is_terminal;
        if let Some(stage_id) = stage_path(ui, case, sequential, enabled) {
            *action = Some(IntakeCaseAction::Transition {
                case_id: case.case.id.clone(),
                stage_id,
                version: case.case.version,
            });
        }
        if sequential && !case.stage.is_terminal {
            ui.label(
                RichText::new("This path only moves to the previous or next stage.")
                    .color(theme::muted())
                    .size(11.0),
            );
        }
        if case.stage.is_terminal {
            ui.label(
                RichText::new("This case is in a terminal stage.")
                    .color(theme::muted())
                    .size(12.0),
            );
        }
        if state.dirty {
            ui.label(
                RichText::new("Save fields before moving the case.")
                    .color(theme::warning())
                    .size(11.0),
            );
        }
        if state.transitioning {
            ui.label(
                RichText::new("Moving case…")
                    .color(theme::muted())
                    .size(11.0),
            );
        }
        if let Some(error) = &state.transition_error {
            ui.colored_label(theme::danger(), error);
        }

        if !case.case.onboarded_at.is_some() && case.case.closed_at.is_none() {
            ui.add_space(12.0);
            if state.cancel_open {
                let label_response =
                    ui.label(RichText::new("Cancellation reason").font(theme::strong_font(13.0)));
                let reason = ui.add_sized(
                    [ui.available_width(), 56.0],
                    egui::TextEdit::multiline(&mut state.cancel_reason)
                        .hint_text("Why is this intake stopping?"),
                );
                reason.labelled_by(label_response.id);
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(
                        !state.cancel_saving
                            && !state.cancel_reason.trim().is_empty()
                            && !state.dirty,
                        |ui| {
                            if ui.button("Confirm cancellation").clicked() {
                                *action = Some(IntakeCaseAction::Cancel {
                                    reason: state.cancel_reason.trim().to_owned(),
                                });
                            }
                        },
                    );
                    if ui.button("Keep case").clicked() {
                        state.cancel_open = false;
                        state.cancel_reason.clear();
                        state.cancel_error = None;
                    }
                    if let Some(error) = &state.cancel_error {
                        ui.colored_label(theme::danger(), error);
                    }
                });
            } else if ui.button("Cancel case").clicked() {
                state.cancel_open = true;
                state.cancel_error = None;
            }
        }
    });
}

fn stage_path(
    ui: &mut Ui,
    case: &ApiIntakeCaseDetail,
    sequential: bool,
    enabled: bool,
) -> Option<String> {
    const ROW_HEIGHT: f32 = 30.0;
    const NODE_SIZE: f32 = 18.0;
    let mut stages: Vec<_> = case
        .flow
        .stages
        .iter()
        .filter(|stage| stage.kind != "cancelled" || stage.id == case.case.stage_id)
        .collect();
    stages.sort_by_key(|stage| stage.sort_order);
    let current_index = stages
        .iter()
        .position(|stage| stage.id == case.case.stage_id);
    let line_x = ui.cursor().left() + NODE_SIZE / 2.0;
    let line_start = ui.cursor().top() + ROW_HEIGHT / 2.0;
    let line_end = line_start + ROW_HEIGHT * stages.len().saturating_sub(1) as f32;
    if stages.len() > 1 {
        let painter = ui.painter();
        painter.line_segment(
            [egui::pos2(line_x, line_start), egui::pos2(line_x, line_end)],
            Stroke::new(2.0, theme::border_strong()),
        );
        if let Some(index) = current_index
            && index > 0
        {
            painter.line_segment(
                [
                    egui::pos2(line_x, line_start),
                    egui::pos2(line_x, line_start + ROW_HEIGHT * index as f32),
                ],
                Stroke::new(2.0, theme::accent()),
            );
        }
    }

    let mut selected = None;
    for (index, stage) in stages.iter().enumerate() {
        let active = Some(index) == current_index;
        let adjacent = current_index.is_some_and(|current| current.abs_diff(index) == 1);
        let clickable = enabled && !active && (!sequential || adjacent);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), ROW_HEIGHT),
            Layout::left_to_right(Align::Center),
            |ui| {
                let (node_rect, node_response) = ui.allocate_exact_size(
                    egui::vec2(NODE_SIZE, NODE_SIZE),
                    if clickable {
                        egui::Sense::click()
                    } else {
                        egui::Sense::hover()
                    },
                );
                let painter = ui.painter();
                let fill = if active {
                    theme::accent()
                } else if clickable {
                    theme::surface_raised()
                } else {
                    theme::surface()
                };
                let stroke = if active || clickable {
                    theme::accent()
                } else {
                    theme::border_strong()
                };
                painter.circle_filled(node_rect.center(), NODE_SIZE / 2.0, fill);
                painter.circle_stroke(
                    node_rect.center(),
                    NODE_SIZE / 2.0,
                    Stroke::new(1.5, stroke),
                );
                if active {
                    painter.circle_filled(node_rect.center(), 4.0, theme::on_accent());
                }
                ui.add_space(7.0);
                let clicked = if active {
                    ui.label(
                        RichText::new(&stage.name)
                            .font(theme::strong_font(12.0))
                            .color(theme::accent()),
                    );
                    theme::pill(ui, "Current", theme::accent());
                    false
                } else if clickable {
                    ui.selectable_label(false, &stage.name).clicked()
                } else {
                    ui.label(RichText::new(&stage.name).color(theme::faint()).size(12.0));
                    false
                };
                if (clicked || node_response.clicked()) && clickable {
                    selected = Some(stage.id.clone());
                }
            },
        );
    }
    selected
}

fn field_editor(
    ui: &mut Ui,
    state: &mut IntakeCaseColumnState,
    field: &ApiIntakeField,
    required: bool,
) {
    let label = format!("{}{}", field.label, if required { " *" } else { "" });
    match field.data_type.as_str() {
        "date" => {
            let value = state.drafts.entry(field.key.clone()).or_default();
            let before = value.clone();
            crate::forms::date_field(ui, &field.key, &label, value);
            if *value != before {
                state.dirty = true;
            }
            return;
        }
        "integer" => {
            let value = state.drafts.entry(field.key.clone()).or_default();
            let before = value.clone();
            crate::forms::integer_field(ui, &label, value);
            if *value != before {
                state.dirty = true;
            }
            return;
        }
        "decimal" => {
            let value = state.drafts.entry(field.key.clone()).or_default();
            let before = value.clone();
            crate::forms::decimal_field(ui, &label, value);
            if *value != before {
                state.dirty = true;
            }
            return;
        }
        "email" => {
            let value = state.drafts.entry(field.key.clone()).or_default();
            let before = value.clone();
            crate::forms::email_field(ui, &label, value);
            if *value != before {
                state.dirty = true;
            }
            return;
        }
        "phone" => {
            let value = state.drafts.entry(field.key.clone()).or_default();
            let before = value.clone();
            crate::forms::phone_field(ui, &label, value);
            if *value != before {
                state.dirty = true;
            }
            return;
        }
        "url" => {
            let value = state.drafts.entry(field.key.clone()).or_default();
            let before = value.clone();
            crate::forms::url_field(ui, &label, value);
            if *value != before {
                state.dirty = true;
            }
            return;
        }
        _ => {}
    }
    ui.add_space(4.0);
    let label_response = ui.label(RichText::new(label).color(theme::muted()).size(11.0));
    match field.data_type.as_str() {
        "boolean" => {
            let value = state.booleans.entry(field.key.clone()).or_insert(false);
            let response = ui.checkbox(value, "Yes");
            let response = response.labelled_by(label_response.id);
            if response.changed() {
                state.dirty = true;
            }
        }
        "select" => {
            let current = state.drafts.get(&field.key).cloned().unwrap_or_default();
            let selected = if current.is_empty() {
                "Select…".to_owned()
            } else {
                current.clone()
            };
            egui::ComboBox::from_id_salt(("intake-field", &field.key))
                .selected_text(selected)
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for option in &field.options {
                        if ui.selectable_label(&current == option, option).clicked() {
                            state.drafts.insert(field.key.clone(), option.clone());
                            state.dirty = true;
                            ui.close();
                        }
                    }
                });
        }
        "long_text" | "multiline" => {
            let value = state.drafts.entry(field.key.clone()).or_default();
            let response = ui.add_sized(
                [ui.available_width(), 64.0],
                egui::TextEdit::multiline(value),
            );
            let response = response.labelled_by(label_response.id);
            if response.changed() {
                state.dirty = true;
            }
        }
        "multiselect" => {
            let value = state.drafts.entry(field.key.clone()).or_default();
            let response = ui.add_sized(
                [ui.available_width(), 30.0],
                egui::TextEdit::singleline(value).hint_text("Separate values with commas"),
            );
            let response = response.labelled_by(label_response.id);
            if response.changed() {
                state.dirty = true;
            }
        }
        _ => {
            let value = state.drafts.entry(field.key.clone()).or_default();
            let response = ui.add_sized(
                [ui.available_width(), 30.0],
                egui::TextEdit::singleline(value),
            );
            let response = response.labelled_by(label_response.id);
            if response.changed() {
                state.dirty = true;
            }
        }
    }
}

fn subject_kind_label(value: Option<&str>) -> &'static str {
    match value {
        Some("client") => "Clients",
        Some("worker") => "Workers",
        Some("other_contact") => "Other contacts",
        Some("organisation") => "Organisations",
        _ => "All types",
    }
}

fn assignee_label(value: &str) -> &'static str {
    match value {
        "me" => "My cases",
        "unassigned" => "Unassigned",
        _ => "Everyone",
    }
}

fn loading(ui: &mut Ui, message: &str) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(RichText::new(message).color(theme::muted()).size(12.0));
    });
}

fn error_card(ui: &mut Ui, title: &str, error: &str) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).font(theme::strong_font(14.0)));
        ui.label(RichText::new(error).color(theme::danger()).size(12.0));
    });
    ui.add_space(6.0);
}

fn humanize_key(key: &str) -> String {
    let key = key.strip_prefix("custom:").unwrap_or(key);
    let mut chars = key.replace('_', " ").chars().collect::<Vec<_>>();
    if let Some(first) = chars.first_mut() {
        *first = first.to_ascii_uppercase();
    }
    chars.into_iter().collect()
}

fn format_value(value: &Value) -> String {
    match value {
        Value::Null => "—".to_owned(),
        Value::Bool(value) => if *value { "Yes" } else { "No" }.to_owned(),
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Array(values) => values
            .iter()
            .map(format_value)
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(_) => "…".to_owned(),
    }
}

fn short_date(value: &str) -> &str {
    value.get(..10).unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::{BOARD_GAP, BOARD_MIN_COLUMN_WIDTH, board_layout};

    #[test]
    fn board_columns_never_overflow_the_available_width() {
        for available in [
            1.0, 120.0, 239.0, 240.0, 300.0, 479.0, 480.0, 700.0, 960.0, 1280.0, 1920.0,
        ] {
            let (per_row, width) = board_layout(available);
            assert!(per_row >= 1);
            if per_row == 1 {
                assert_eq!(width, available);
            } else {
                assert!(width >= BOARD_MIN_COLUMN_WIDTH);
                let row_width = width * per_row as f32 + BOARD_GAP * (per_row - 1) as f32;
                assert!(
                    row_width <= available + 0.01,
                    "row of {per_row} columns is {row_width} wide in {available}"
                );
            }
        }
    }

    #[test]
    fn board_columns_reflow_from_four_to_two_to_one() {
        assert_eq!(board_layout(1080.0).0, 4);
        assert_eq!(board_layout(560.0).0, 2);
        assert_eq!(board_layout(300.0).0, 1);
    }
}
