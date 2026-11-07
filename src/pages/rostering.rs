//! Rostering: shifts, client and worker assignments, and explainable
//! worker suitability from the server's rostering graph.

use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea, Stroke, Ui,
    vec2,
};

use crate::{
    contact::{ApiGraphNode, PartyKind},
    rostering::{
        ApiAssignment, ApiAssignmentUpdate, ApiShift, ApiShiftDetail, ApiShiftUpdate,
        ApiShiftWrite, ApiSite, ApiSiteWrite, ApiSuitability, local_date_key, local_inputs,
        local_range, local_to_utc, parse_utc, shift_kind_label, status_label,
    },
    theme,
};

pub struct RosteringState {
    pub sites: Vec<ApiSite>,
    pub shifts: Vec<ApiShift>,
    pub detail: Option<ApiShiftDetail>,
    pub suitability: Option<ApiSuitability>,
    pub selected: Option<String>,
    pub loading_shifts: bool,
    pub loading_detail: bool,
    pub loading_suitability: bool,
    pub error: Option<String>,
    pub filter_from: String,
    pub filter_to: String,
    pub filter_status: String,
    pub filter_site: Option<String>,
    pub requested: bool,
    pub editor: Option<ShiftEditor>,
    pub site_form: Option<SiteForm>,
    pub assignment_role: String,
    pub assignment_target: Option<ApiGraphNode>,
    pub assignment_error: Option<String>,
    pub suitability_error: Option<String>,
    pub assignment_pending: bool,
}

impl Default for RosteringState {
    fn default() -> Self {
        let today = chrono::Local::now().date_naive();
        Self {
            sites: Vec::new(),
            shifts: Vec::new(),
            detail: None,
            suitability: None,
            selected: None,
            loading_shifts: false,
            loading_detail: false,
            loading_suitability: false,
            error: None,
            filter_from: (today - chrono::Duration::days(30))
                .format("%Y-%m-%d")
                .to_string(),
            filter_to: (today + chrono::Duration::days(30))
                .format("%Y-%m-%d")
                .to_string(),
            filter_status: "all".to_owned(),
            filter_site: None,
            requested: false,
            editor: None,
            site_form: None,
            assignment_role: "worker".to_owned(),
            assignment_target: None,
            assignment_error: None,
            suitability_error: None,
            assignment_pending: false,
        }
    }
}

pub struct ShiftEditor {
    pub editing_id: Option<String>,
    pub closed: bool,
    pub title: String,
    pub kind: String,
    pub status: String,
    pub date: String,
    pub start: String,
    pub end: String,
    pub site_id: Option<String>,
    pub location: String,
    pub notes: String,
    pub error: Option<String>,
    pub saving: bool,
}

impl ShiftEditor {
    pub fn new_for(date: String) -> Self {
        Self {
            editing_id: None,
            closed: false,
            title: String::new(),
            kind: "service_delivery".to_owned(),
            status: "draft".to_owned(),
            date,
            start: "09:00".to_owned(),
            end: "11:00".to_owned(),
            site_id: None,
            location: String::new(),
            notes: String::new(),
            error: None,
            saving: false,
        }
    }

    pub fn from_shift(shift: &ApiShift) -> Self {
        let (date, start) = local_inputs(&shift.starts_at);
        let (_, end) = local_inputs(&shift.ends_at);
        Self {
            editing_id: Some(shift.id.clone()),
            closed: false,
            title: shift.title.clone().unwrap_or_default(),
            kind: shift.kind.clone(),
            status: shift.status.clone(),
            date,
            start,
            end,
            site_id: shift.site_id.clone(),
            location: shift.location.clone().unwrap_or_default(),
            notes: String::new(),
            error: None,
            saving: false,
        }
    }

    fn build(&self) -> Result<(String, String), String> {
        let starts_at = local_to_utc(&self.date, &self.start)?;
        let ends_at = local_to_utc(&self.date, &self.end)?;
        if let (Some(start), Some(end)) = (parse_utc(&starts_at), parse_utc(&ends_at))
            && end <= start
        {
            return Err("End time must be after the start time.".to_owned());
        }
        Ok((starts_at, ends_at))
    }
}

#[derive(Default)]
pub struct SiteForm {
    pub name: String,
    pub address: String,
    pub error: Option<String>,
    pub saving: bool,
    pub closed: bool,
}

pub enum RosteringAction {
    OpenCreate(String),
    Refresh,
    LoadShift(String),
    CreateShift(ApiShiftWrite),
    UpdateShift {
        id: String,
        payload: ApiShiftUpdate,
    },
    CreateSite(ApiSiteWrite),
    CreateAssignment {
        shift_id: String,
        contact_id: String,
        role: String,
    },
    UpdateAssignment {
        id: String,
        payload: ApiAssignmentUpdate,
    },
    DeleteAssignment(String),
    FindWorkers(String),
}

pub fn show(
    ui: &mut Ui,
    state: &mut RosteringState,
    catalog: &[ApiGraphNode],
) -> Option<RosteringAction> {
    let mut action = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            header(ui, state, &mut action);
            ui.add_space(16.0);
            filters(ui, state, &mut action);
            ui.add_space(12.0);

            let mut close_editor = false;
            if let Some(editor) = &mut state.editor {
                editor_card(ui, editor, &state.sites, &mut action);
                close_editor = editor.closed;
                ui.add_space(12.0);
            }
            if close_editor {
                state.editor = None;
            }

            let mut close_site = false;
            if let Some(site_form) = &mut state.site_form {
                site_card(ui, site_form, &mut action);
                close_site = site_form.closed;
                ui.add_space(12.0);
            }
            if close_site {
                state.site_form = None;
            }

            if state.error.is_some() {
                if let Some(error) = &state.error {
                    error_box(ui, error);
                }
                ui.add_space(10.0);
            }

            let wide = ui.available_width() > 940.0;
            if wide {
                ui.columns(2, |cols| {
                    shift_list(&mut cols[0], state, &mut action);
                    shift_detail(&mut cols[1], state, catalog, &mut action);
                });
            } else {
                shift_list(ui, state, &mut action);
                ui.add_space(12.0);
                shift_detail(ui, state, catalog, &mut action);
            }

            ui.add_space(24.0);
        });

    action
}

fn header(ui: &mut Ui, state: &RosteringState, action: &mut Option<RosteringAction>) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(52.0, 52.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, CornerRadius::same(14), theme::surface_raised());
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "📅",
            egui::FontId::proportional(22.0),
            theme::text(),
        );
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(RichText::new("Rostering").font(theme::strong_font(26.0)));
            ui.label(
                RichText::new("Shifts, assignments and explainable worker suitability.")
                    .color(theme::muted())
                    .size(13.0),
            );
        });

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.add(theme::primary_button("+ New shift")).clicked() {
                let today = chrono::Local::now()
                    .date_naive()
                    .format("%Y-%m-%d")
                    .to_string();
                *action = Some(RosteringAction::OpenCreate(today));
            }
            if ui.add(theme::secondary_button("Refresh")).clicked() {
                *action = Some(RosteringAction::Refresh);
            }
            if state.loading_shifts {
                ui.label(RichText::new("Loading…").color(theme::warning()).size(11.0));
            }
        });
    });
}

fn filters(ui: &mut Ui, state: &mut RosteringState, action: &mut Option<RosteringAction>) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            field_sized(ui, "From", &mut state.filter_from, 96.0);
            field_sized(ui, "To", &mut state.filter_to, 96.0);

            ui.label(RichText::new("Status").color(theme::muted()).size(10.5));
            let status_filter = egui::ComboBox::from_id_salt("roster-status")
                .selected_text(status_label(&state.filter_status))
                .width(120.0)
                .show_ui(ui, |ui| {
                    for status in ["all", "draft", "published", "completed", "cancelled"] {
                        ui.selectable_value(
                            &mut state.filter_status,
                            status.to_owned(),
                            status_label(status),
                        );
                    }
                });
            name_combo(&status_filter.response, "Status filter");

            ui.label(RichText::new("Site").color(theme::muted()).size(10.5));
            let site_label = state
                .filter_site
                .as_ref()
                .and_then(|id| state.sites.iter().find(|site| &site.id == id))
                .map(|site| site.name.clone())
                .unwrap_or_else(|| "All sites".to_owned());
            let site_filter = egui::ComboBox::from_id_salt("roster-site-filter")
                .selected_text(site_label)
                .width(180.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(state.filter_site.is_none(), "All sites")
                        .clicked()
                    {
                        state.filter_site = None;
                    }
                    for site in &state.sites {
                        let selected = state.filter_site.as_deref() == Some(site.id.as_str());
                        if ui.selectable_label(selected, &site.name).clicked() {
                            state.filter_site = Some(site.id.clone());
                        }
                    }
                });
            name_combo(&site_filter.response, "Site filter");

            if ui.add(theme::secondary_button("Apply filters")).clicked() {
                *action = Some(RosteringAction::Refresh);
            }
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("+ Add site")
                            .color(theme::accent())
                            .size(10.5),
                    )
                    .frame(false),
                )
                .clicked()
            {
                state.site_form = Some(SiteForm::default());
            }
        });
    });
}

fn shift_list(ui: &mut Ui, state: &mut RosteringState, action: &mut Option<RosteringAction>) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Shifts").font(theme::strong_font(15.0)));
            theme::pill(ui, &format!("{}", state.shifts.len()), theme::accent());
        });
        ui.add_space(6.0);

        if state.loading_shifts && state.shifts.is_empty() {
            ui.label(
                RichText::new("Loading shifts…")
                    .color(theme::muted())
                    .size(11.0),
            );
            return;
        }
        if state.shifts.is_empty() {
            ui.label(
                RichText::new("No shifts match these filters.")
                    .color(theme::faint())
                    .size(11.0),
            );
            return;
        }

        ScrollArea::vertical()
            .id_salt("roster_shift_list")
            .auto_shrink([false, false])
            .max_height(620.0)
            .show(ui, |ui| {
                let mut current_date = String::new();
                for shift in &state.shifts {
                    let date = local_date_key(&shift.starts_at);
                    if date != current_date {
                        current_date = date.clone();
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(crate::rostering::local_date(&shift.starts_at))
                                .color(theme::faint())
                                .font(theme::strong_font(10.0)),
                        );
                    }
                    shift_row(ui, shift, state.selected.as_deref(), action);
                    ui.add_space(3.0);
                }
            });
    });
}

fn shift_row(
    ui: &mut Ui,
    shift: &ApiShift,
    selected: Option<&str>,
    action: &mut Option<RosteringAction>,
) {
    let is_selected = selected == Some(shift.id.as_str());
    let response = Frame::new()
        .fill(if is_selected {
            theme::accent_soft(20)
        } else {
            theme::surface_raised()
        })
        .stroke(Stroke::new(
            1.0,
            if is_selected {
                theme::accent().gamma_multiply(0.6)
            } else {
                theme::border()
            },
        ))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::same(9))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(local_range(&shift.starts_at, &shift.ends_at))
                        .color(theme::text())
                        .font(theme::strong_font(11.5)),
                );
                status_pill(ui, &shift.status);
                if shift.client_count > 0 {
                    ui.label(
                        RichText::new(format!("{} client", shift.client_count))
                            .color(theme::faint())
                            .size(10.0),
                    );
                }
                if shift.worker_count > 0 {
                    ui.label(
                        RichText::new(format!("{} worker", shift.worker_count))
                            .color(theme::faint())
                            .size(10.0),
                    );
                }
            });
            ui.label(
                RichText::new(
                    shift
                        .title
                        .clone()
                        .or_else(|| shift.service_name.clone())
                        .unwrap_or_else(|| shift_kind_label(&shift.kind)),
                )
                .color(theme::text())
                .size(12.0),
            );
            let context = [shift.site_name.as_deref(), shift.location.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
            if !context.is_empty() {
                ui.label(RichText::new(context).color(theme::muted()).size(10.0));
            }
        })
        .response;

    if response.interact(egui::Sense::click()).clicked() {
        *action = Some(RosteringAction::LoadShift(shift.id.clone()));
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

fn shift_detail(
    ui: &mut Ui,
    state: &mut RosteringState,
    catalog: &[ApiGraphNode],
    action: &mut Option<RosteringAction>,
) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());

        let Some(detail) = state.detail.clone() else {
            ui.label(RichText::new("Shift detail").font(theme::strong_font(15.0)));
            ui.label(
                RichText::new("Select a shift to see assignments and suitability.")
                    .color(theme::faint())
                    .size(11.0),
            );
            return;
        };

        let shift = &detail.shift;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(
                    shift
                        .title
                        .clone()
                        .or_else(|| shift.service_name.clone())
                        .unwrap_or_else(|| shift_kind_label(&shift.kind)),
                )
                .font(theme::strong_font(17.0)),
            );
            status_pill(ui, &shift.status);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Edit").color(theme::accent()).size(11.0))
                            .frame(false),
                    )
                    .clicked()
                {
                    state.editor = Some(ShiftEditor::from_shift(shift));
                }
            });
        });
        ui.label(
            RichText::new(format!(
                "{} · {}",
                crate::rostering::local_date(&shift.starts_at),
                local_range(&shift.starts_at, &shift.ends_at)
            ))
            .color(theme::muted())
            .size(11.0),
        );
        ui.add_space(4.0);
        meta_row(ui, "Kind", &shift_kind_label(&shift.kind));
        if let Some(site) = &shift.site_name {
            meta_row(ui, "Site", site);
        }
        if let Some(service) = &shift.service_name {
            meta_row(ui, "Service", service);
        }
        if let Some(division) = &shift.division_name {
            meta_row(ui, "Division", division);
        }
        if let Some(location) = &shift.location {
            meta_row(ui, "Location", location);
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        ui.label(RichText::new("Assignments").font(theme::strong_font(13.0)));
        if detail.assignments.is_empty() {
            ui.label(
                RichText::new("No one is assigned yet.")
                    .color(theme::faint())
                    .size(10.5),
            );
        }
        for assignment in &detail.assignments {
            assignment_row(ui, assignment, action);
        }

        ui.add_space(6.0);
        assignment_add(ui, shift, state, catalog, action);

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(8.0);
        suitability_section(ui, state, action);
    });
}

fn assignment_row(ui: &mut Ui, assignment: &ApiAssignment, action: &mut Option<RosteringAction>) {
    Frame::new()
        .fill(theme::surface_raised())
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let role_label = if assignment.role == "worker" {
                    "Worker"
                } else {
                    "Client"
                };
                ui.label(
                    RichText::new(&assignment.contact_name)
                        .color(theme::text())
                        .font(theme::strong_font(11.5)),
                );
                theme::pill(ui, role_label, theme::accent_blue());

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Remove").color(theme::faint()).size(9.5),
                            )
                            .frame(false),
                        )
                        .clicked()
                    {
                        *action = Some(RosteringAction::DeleteAssignment(assignment.id.clone()));
                    }
                });
            });

            ui.horizontal_wrapped(|ui| {
                let mut status = assignment.status.clone();
                let status_combo =
                    egui::ComboBox::from_id_salt(("assignment-status", &assignment.id))
                        .selected_text(status_label(&status))
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            for option in ["assigned", "attended", "no_show", "cancelled"] {
                                ui.selectable_value(
                                    &mut status,
                                    option.to_owned(),
                                    status_label(option),
                                );
                            }
                        });
                name_combo(&status_combo.response, "Assignment status");
                if status != assignment.status {
                    *action = Some(RosteringAction::UpdateAssignment {
                        id: assignment.id.clone(),
                        payload: ApiAssignmentUpdate {
                            status: Some(status),
                            outcome: None,
                            outcome_rating: None,
                            notes: None,
                        },
                    });
                }

                if assignment.role == "worker" {
                    let mut outcome = assignment.outcome.clone().unwrap_or_default();
                    let outcome_combo =
                        egui::ComboBox::from_id_salt(("assignment-outcome", &assignment.id))
                            .selected_text(if outcome.is_empty() {
                                "Outcome".to_owned()
                            } else {
                                status_label(&outcome)
                            })
                            .width(110.0)
                            .show_ui(ui, |ui| {
                                for option in ["positive", "neutral", "negative"] {
                                    ui.selectable_value(
                                        &mut outcome,
                                        option.to_owned(),
                                        status_label(option),
                                    );
                                }
                            });
                    name_combo(&outcome_combo.response, "Assignment outcome");
                    if outcome != assignment.outcome.clone().unwrap_or_default()
                        && !outcome.is_empty()
                    {
                        *action = Some(RosteringAction::UpdateAssignment {
                            id: assignment.id.clone(),
                            payload: ApiAssignmentUpdate {
                                status: None,
                                outcome: Some(outcome),
                                outcome_rating: None,
                                notes: None,
                            },
                        });
                    }

                    let mut rating = assignment.outcome_rating.unwrap_or(0);
                    let rating_combo =
                        egui::ComboBox::from_id_salt(("assignment-rating", &assignment.id))
                            .selected_text(if rating == 0 {
                                "Rating".to_owned()
                            } else {
                                format!("{rating} / 5")
                            })
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                for value in 1..=5 {
                                    ui.selectable_value(&mut rating, value, format!("{value} / 5"));
                                }
                            });
                    name_combo(&rating_combo.response, "Assignment rating");
                    if rating != assignment.outcome_rating.unwrap_or(0) && rating > 0 {
                        *action = Some(RosteringAction::UpdateAssignment {
                            id: assignment.id.clone(),
                            payload: ApiAssignmentUpdate {
                                status: None,
                                outcome: None,
                                outcome_rating: Some(rating),
                                notes: None,
                            },
                        });
                    }
                }
            });
        });
    ui.add_space(4.0);
}

fn assignment_add(
    ui: &mut Ui,
    shift: &ApiShift,
    state: &mut RosteringState,
    catalog: &[ApiGraphNode],
    action: &mut Option<RosteringAction>,
) {
    ui.add_space(4.0);
    ui.label(
        RichText::new("Add assignment")
            .color(theme::muted())
            .font(theme::strong_font(10.5)),
    );
    ui.horizontal_wrapped(|ui| {
        let label = state
            .assignment_target
            .as_ref()
            .map(|node| node.label.clone())
            .unwrap_or_else(|| "Select a contact…".to_owned());
        let contact_combo = egui::ComboBox::from_id_salt("assignment-contact")
            .selected_text(label)
            .width(220.0)
            .show_ui(ui, |ui| {
                let search_id = ui.scope_id().with("assignment-search");
                let mut search =
                    ui.data_mut(|data| data.get_temp::<String>(search_id).unwrap_or_default());
                ui.add(
                    egui::TextEdit::singleline(&mut search)
                        .hint_text("Search contacts…")
                        .desired_width(220.0),
                );
                let needle = search.trim().to_lowercase();
                let mut chosen = None;
                ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    let mut shown = 0;
                    for node in catalog
                        .iter()
                        .filter(|node| node.kind == PartyKind::Contact)
                    {
                        if !needle.is_empty() && !node.label.to_lowercase().contains(&needle) {
                            continue;
                        }
                        if shown >= 50 {
                            break;
                        }
                        shown += 1;
                        let selected = state
                            .assignment_target
                            .as_ref()
                            .is_some_and(|target| target.id == node.id);
                        if ui.selectable_label(selected, &node.label).clicked() {
                            chosen = Some(node.clone());
                            ui.close();
                        }
                    }
                    if shown == 0 {
                        ui.label("No matching contacts");
                    }
                });
                ui.data_mut(|data| data.insert_temp(search_id, search));
                if let Some(node) = chosen {
                    state.assignment_target = Some(node);
                }
            });
        name_combo(&contact_combo.response, "Assignment contact");

        let role_combo = egui::ComboBox::from_id_salt("assignment-role")
            .selected_text(if state.assignment_role == "worker" {
                "Worker"
            } else {
                "Client"
            })
            .width(100.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.assignment_role, "worker".to_owned(), "Worker");
                ui.selectable_value(&mut state.assignment_role, "client".to_owned(), "Client");
            });
        name_combo(&role_combo.response, "Assignment role");

        let ready = state.assignment_target.is_some() && !state.assignment_pending;
        if ui
            .add_enabled(ready, theme::secondary_button("Assign"))
            .clicked()
            && let Some(target) = &state.assignment_target
        {
            state.assignment_pending = true;
            state.assignment_error = None;
            *action = Some(RosteringAction::CreateAssignment {
                shift_id: shift.id.clone(),
                contact_id: target.id.clone(),
                role: state.assignment_role.clone(),
            });
        }
    });
    if let Some(error) = &state.assignment_error {
        ui.label(RichText::new(error).color(theme::danger()).size(10.0));
    }
}

fn suitability_section(
    ui: &mut Ui,
    state: &mut RosteringState,
    action: &mut Option<RosteringAction>,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Worker suitability").font(theme::strong_font(13.0)));
        ui.label(
            RichText::new("Advisory — every score explains itself")
                .color(theme::faint())
                .size(9.5),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let shift_id = state.detail.as_ref().map(|detail| detail.shift.id.clone());
            if ui
                .add(theme::secondary_button("Find suitable workers"))
                .clicked()
                && let Some(shift_id) = shift_id
            {
                state.loading_suitability = true;
                state.suitability_error = None;
                *action = Some(RosteringAction::FindWorkers(shift_id));
            }
        });
    });

    if state.loading_suitability {
        ui.label(
            RichText::new("Scoring workers…")
                .color(theme::muted())
                .size(10.5),
        );
    }
    if let Some(error) = &state.suitability_error {
        ui.label(RichText::new(error).color(theme::danger()).size(10.0));
    }

    let Some(suitability) = state.suitability.clone() else {
        return;
    };

    ui.add_space(4.0);
    for candidate in &suitability.candidates {
        Frame::new()
            .fill(theme::surface_raised())
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&candidate.display_name)
                            .color(theme::text())
                            .font(theme::strong_font(11.5)),
                    );
                    theme::pill(ui, &format!("{:.0}", candidate.score), theme::success());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Assign").color(theme::accent()).size(10.0),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            state.assignment_target = Some(ApiGraphNode {
                                kind: PartyKind::Contact,
                                id: candidate.contact_id.clone(),
                                label: candidate.display_name.clone(),
                                degree: 0,
                                depth: None,
                            });
                            state.assignment_role = "worker".to_owned();
                            state.assignment_pending = true;
                            *action = Some(RosteringAction::CreateAssignment {
                                shift_id: suitability.shift_id.clone(),
                                contact_id: candidate.contact_id.clone(),
                                role: "worker".to_owned(),
                            });
                        }
                    });
                });
                for reason in &candidate.reasons {
                    ui.label(
                        RichText::new(format!("+ {reason}"))
                            .color(theme::muted())
                            .size(9.5),
                    );
                }
                for warning in &candidate.warnings {
                    ui.label(
                        RichText::new(format!("! {warning}"))
                            .color(theme::warning())
                            .size(9.5),
                    );
                }
            });
        ui.add_space(4.0);
    }

    if !suitability.blocked.is_empty() {
        ui.add_space(4.0);
        ui.label(
            RichText::new("Blocked by constraints")
                .color(theme::danger())
                .font(theme::strong_font(10.5)),
        );
        for candidate in &suitability.blocked {
            ui.label(
                RichText::new(format!(
                    "{} — {}",
                    candidate.display_name,
                    candidate.warnings.join("; ")
                ))
                .color(theme::muted())
                .size(10.0),
            );
        }
    }
}

fn editor_card(
    ui: &mut Ui,
    editor: &mut ShiftEditor,
    sites: &[ApiSite],
    action: &mut Option<RosteringAction>,
) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(if editor.editing_id.is_some() {
                    "Edit shift"
                } else {
                    "New shift"
                })
                .font(theme::strong_font(15.0)),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Close").color(theme::faint()).size(10.5))
                            .frame(false),
                    )
                    .clicked()
                {
                    editor.closed = true;
                }
            });
        });

        ui.horizontal_wrapped(|ui| {
            field_sized(ui, "Title", &mut editor.title, 180.0);
            ui.label(RichText::new("Kind").color(theme::muted()).size(10.5));
            let kind = egui::ComboBox::from_id_salt("shift-kind")
                .selected_text(shift_kind_label(&editor.kind))
                .width(150.0)
                .show_ui(ui, |ui| {
                    for kind in ["service_delivery", "opening_hours", "respite_program"] {
                        ui.selectable_value(
                            &mut editor.kind,
                            kind.to_owned(),
                            shift_kind_label(kind),
                        );
                    }
                });
            name_combo(&kind.response, "Shift kind");
            ui.label(RichText::new("Status").color(theme::muted()).size(10.5));
            let status = egui::ComboBox::from_id_salt("shift-status")
                .selected_text(status_label(&editor.status))
                .width(110.0)
                .show_ui(ui, |ui| {
                    for status in ["draft", "published", "cancelled", "completed"] {
                        ui.selectable_value(
                            &mut editor.status,
                            status.to_owned(),
                            status_label(status),
                        );
                    }
                });
            name_combo(&status.response, "Shift status");
        });

        ui.horizontal_wrapped(|ui| {
            field_sized(ui, "Date", &mut editor.date, 110.0);
            field_sized(ui, "Start", &mut editor.start, 70.0);
            field_sized(ui, "End", &mut editor.end, 70.0);

            ui.label(RichText::new("Site").color(theme::muted()).size(10.5));
            let site_label = editor
                .site_id
                .as_ref()
                .and_then(|id| sites.iter().find(|site| &site.id == id))
                .map(|site| site.name.clone())
                .unwrap_or_else(|| "No site".to_owned());
            let site = egui::ComboBox::from_id_salt("shift-site")
                .selected_text(site_label)
                .width(180.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(editor.site_id.is_none(), "No site")
                        .clicked()
                    {
                        editor.site_id = None;
                    }
                    for site in sites {
                        let selected = editor.site_id.as_deref() == Some(site.id.as_str());
                        if ui.selectable_label(selected, &site.name).clicked() {
                            editor.site_id = Some(site.id.clone());
                        }
                    }
                });
            name_combo(&site.response, "Shift site");
            field_sized(ui, "Location", &mut editor.location, 220.0);
        });
        field_sized(ui, "Notes", &mut editor.notes, 420.0);

        if let Some(error) = &editor.error
            && !error.is_empty()
        {
            ui.label(RichText::new(error).color(theme::danger()).size(10.5));
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let label = if editor.saving {
                "Saving…"
            } else {
                "Save shift"
            };
            if ui
                .add_enabled(!editor.saving, theme::primary_button(label))
                .clicked()
            {
                match editor.build() {
                    Ok((starts_at, ends_at)) => {
                        editor.error = None;
                        editor.saving = true;
                        let title = trim_opt(&editor.title);
                        let location = trim_opt(&editor.location);
                        let notes = trim_opt(&editor.notes);
                        if let Some(id) = &editor.editing_id {
                            *action = Some(RosteringAction::UpdateShift {
                                id: id.clone(),
                                payload: ApiShiftUpdate {
                                    title,
                                    kind: Some(editor.kind.clone()),
                                    starts_at: Some(starts_at),
                                    ends_at: Some(ends_at),
                                    location,
                                    status: Some(editor.status.clone()),
                                    division_id: None,
                                    service_id: None,
                                    site_id: editor.site_id.clone(),
                                    notes,
                                },
                            });
                        } else {
                            *action = Some(RosteringAction::CreateShift(ApiShiftWrite {
                                title,
                                kind: Some(editor.kind.clone()),
                                starts_at,
                                ends_at,
                                location,
                                status: Some(editor.status.clone()),
                                division_id: None,
                                service_id: None,
                                site_id: editor.site_id.clone(),
                                notes,
                            }));
                        }
                    }
                    Err(error) => editor.error = Some(error),
                }
            }
            if ui.add(theme::secondary_button("Cancel")).clicked() {
                editor.closed = true;
            }
        });
    });
}

fn site_card(ui: &mut Ui, form: &mut SiteForm, action: &mut Option<RosteringAction>) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("New site").font(theme::strong_font(15.0)));
        ui.horizontal_wrapped(|ui| {
            field_sized(ui, "Name", &mut form.name, 220.0);
            field_sized(ui, "Address", &mut form.address, 280.0);
            if ui
                .add_enabled(!form.saving, theme::primary_button("Add site"))
                .clicked()
            {
                if form.name.trim().is_empty() {
                    form.error = Some("A site needs a name.".to_owned());
                } else {
                    form.saving = true;
                    form.error = None;
                    *action = Some(RosteringAction::CreateSite(ApiSiteWrite {
                        name: form.name.trim().to_owned(),
                        division_id: None,
                        address: trim_opt(&form.address),
                        notes: None,
                        is_active: Some(true),
                    }));
                }
            }
            if ui.add(theme::secondary_button("Cancel")).clicked() {
                form.closed = true;
            }
        });
        if let Some(error) = &form.error
            && !error.is_empty()
        {
            ui.label(RichText::new(error).color(theme::danger()).size(10.5));
        }
    });
}

fn meta_row(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(theme::faint()).size(10.0));
        ui.label(RichText::new(value).color(theme::text()).size(10.5));
    });
}

fn status_pill(ui: &mut Ui, status: &str) {
    let colour = match status {
        "published" => theme::success(),
        "draft" => theme::warning(),
        "cancelled" => theme::danger(),
        "completed" => theme::accent_blue(),
        _ => theme::muted(),
    };
    theme::pill(ui, &status_label(status).to_uppercase(), colour);
}

fn field_sized(ui: &mut Ui, label: &str, value: &mut String, width: f32) {
    ui.label(RichText::new(label).color(theme::muted()).size(10.5));
    let response = ui.add_sized(
        [width, 30.0],
        egui::TextEdit::singleline(value).margin(Margin::symmetric(8, 6)),
    );
    name_input(&response, label);
}

fn name_input(response: &egui::Response, label: &str) {
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
}

fn name_combo(response: &egui::Response, label: &str) {
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, label));
}

fn trim_opt(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn error_box(ui: &mut Ui, message: &str) {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(
            theme::danger().r(),
            theme::danger().g(),
            theme::danger().b(),
            20,
        ))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.label(RichText::new(message).color(theme::danger()).size(11.0));
        });
}

#[cfg(test)]
mod tests {
    use super::ShiftEditor;

    #[test]
    fn editor_rejects_reversed_times() {
        let mut editor = ShiftEditor::new_for("2026-06-15".to_owned());
        editor.start = "11:00".to_owned();
        editor.end = "09:00".to_owned();
        assert!(editor.build().is_err());

        editor.end = "12:00".to_owned();
        assert!(editor.build().is_ok());
    }
}
