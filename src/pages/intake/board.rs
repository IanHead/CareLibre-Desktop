//! Server-backed intake kanban board and case detail.

use std::collections::BTreeSet;

use eframe::egui::{self, Align, Layout, RichText, ScrollArea, Stroke, Ui};
use serde_json::Value;

use crate::{
    intake_board::{ApiIntakeBoard, ApiIntakeCard, ApiIntakeCaseDetail, ApiIntakeFlow},
    theme,
};

#[derive(Default)]
pub struct IntakeBoardState {
    pub flows: Vec<ApiIntakeFlow>,
    pub flows_loading: bool,
    pub flows_requested: bool,
    pub flows_error: Option<String>,
    pub flows_request_id: Option<u64>,
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
        }
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
}

pub enum IntakeCaseAction {
    Reload,
    Transition {
        case_id: String,
        stage_id: String,
        version: i32,
    },
    OpenContact(String),
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
                let label = match flow.variant.as_deref() {
                    Some(variant) if !variant.is_empty() => {
                        format!("{} · {variant}", flow.name)
                    }
                    _ => flow.name.clone(),
                };
                if ui.checkbox(&mut selected, label).changed() {
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
    });
}

fn board_columns(ui: &mut Ui, board: &ApiIntakeBoard, action: &mut Option<IntakeBoardAction>) {
    let multi_flow = board.flows.len() > 1;
    ScrollArea::horizontal()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for column in &board.columns {
                    ui.vertical(|ui| {
                        ui.set_width(280.0);
                        theme::card().show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&column.stage.name)
                                        .font(theme::strong_font(13.0)),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    theme::pill(ui, &column.count.to_string(), theme::accent());
                                });
                            });
                            ui.add_space(4.0);
                            if column.cards.is_empty() {
                                ui.label(
                                    RichText::new("No cases").color(theme::faint()).size(11.0),
                                );
                            }
                            for card in &column.cards {
                                if card_button(ui, card, multi_flow) {
                                    *action = Some(IntakeBoardAction::OpenCase(card.id.clone()));
                                }
                            }
                        });
                    });
                    ui.add_space(4.0);
                }
            });
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
    case_body(ui, state, &case, &mut action);
    action
}

fn case_body(
    ui: &mut Ui,
    state: &IntakeCaseColumnState,
    case: &ApiIntakeCaseDetail,
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
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!("{} · {}", contact.display_name, contact.role))
                        .color(theme::muted())
                        .size(12.0),
                );
                if ui.small_button("Open contact").clicked() {
                    *action = Some(IntakeCaseAction::OpenContact(contact.id.clone()));
                }
            });
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
        if let Some(data) = case.case.data.as_object() {
            if data.is_empty() {
                ui.label(
                    RichText::new("No fields captured yet.")
                        .color(theme::faint())
                        .size(11.0),
                );
            }
            for (key, value) in data {
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
            ui.horizontal_wrapped(|ui| {
                let mark = if item.completed_at.is_some() {
                    "✔"
                } else {
                    "○"
                };
                let suffix = if item.is_required { " · required" } else { "" };
                ui.label(
                    RichText::new(format!("{mark} {}{suffix}", item.label))
                        .size(12.0)
                        .color(if item.completed_at.is_some() {
                            theme::success()
                        } else {
                            theme::text()
                        }),
                );
            });
            if let Some(description) = &item.description {
                ui.label(RichText::new(description).color(theme::faint()).size(11.0));
            }
        }

        ui.add_space(12.0);
        ui.label(RichText::new("Move to stage").font(theme::strong_font(14.0)));
        if case.allowed_transitions.is_empty() {
            ui.label(
                RichText::new("This case is in a terminal stage.")
                    .color(theme::muted())
                    .size(12.0),
            );
        }
        ui.add_enabled_ui(!state.transitioning, |ui| {
            ui.horizontal_wrapped(|ui| {
                for stage in &case.allowed_transitions {
                    if ui
                        .button(format!("→ {}", stage.name))
                        .on_hover_text(format!("Move to {}", stage.name))
                        .clicked()
                    {
                        *action = Some(IntakeCaseAction::Transition {
                            case_id: case.case.id.clone(),
                            stage_id: stage.id.clone(),
                            version: case.case.version,
                        });
                    }
                }
            });
        });
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
    });
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
