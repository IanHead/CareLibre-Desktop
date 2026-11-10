//! Intake picker and guided onboarding wizards.

pub(crate) mod fields;

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use chrono::NaiveDate;
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, RichText, Sense,
    Stroke, Ui, vec2,
};

use crate::contact::{ApiGraphNode, RoleOption, relationship_role_label};
use crate::intake::{CompletedIntake, IntakeDraft, IntakeKind, IntakeRelationship, IntakeSummary};
use crate::pages::contact_editor::{NewRelationshipContact, party_lookup, role_picker};
use crate::store::ContactFilter;
use crate::theme;

use fields::{FieldKind, FieldSpec, RelationshipSpec, StepSpec};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IntakeView {
    Picker,
    Wizard,
    Completed,
}

#[derive(Clone, Default)]
pub struct IntakeRelationshipForm {
    pub role: Option<RoleOption>,
    pub target: Option<ApiGraphNode>,
    pub notes: String,
}

#[derive(Clone)]
pub struct IntakeNewRelationship {
    pub slot: String,
    pub role: RoleOption,
    pub notes: String,
    pub contact: NewRelationshipContact,
}

pub struct IntakeState {
    pub view: IntakeView,
    pub drafts: Vec<IntakeSummary>,
    pub draft: Option<IntakeDraft>,
    pub completed: Option<CompletedIntake>,
    pub error: Option<String>,
    pub step_error: bool,
    pub saving: bool,
    pub dirty: bool,
    pub save_due: Option<Instant>,
    pub list_requested: bool,
    pub list_loading: bool,
    pub loading_draft: bool,
    pub request_seq: u64,
    pub list_request: Option<u64>,
    pub load_request: Option<u64>,
    pub discard_request: Option<u64>,
    pub create_request: Option<u64>,
    pub popped_out: bool,
    pub relationship_forms: BTreeMap<String, IntakeRelationshipForm>,
    pub new_relationship: Option<IntakeNewRelationship>,
    pub relationship_pending: bool,
    pub relationship_error: Option<String>,
}

impl IntakeState {
    pub fn new() -> Self {
        Self {
            view: IntakeView::Picker,
            drafts: Vec::new(),
            draft: None,
            completed: None,
            error: None,
            step_error: false,
            saving: false,
            dirty: false,
            save_due: None,
            list_requested: false,
            list_loading: false,
            loading_draft: false,
            request_seq: 0,
            list_request: None,
            load_request: None,
            discard_request: None,
            create_request: None,
            popped_out: false,
            relationship_forms: BTreeMap::new(),
            new_relationship: None,
            relationship_pending: false,
            relationship_error: None,
        }
    }

    pub fn reset_relationships(&mut self) {
        self.relationship_forms.clear();
        self.new_relationship = None;
        self.relationship_pending = false;
        self.relationship_error = None;
    }

    pub fn next_request(&mut self) -> u64 {
        self.request_seq += 1;
        self.request_seq
    }

    pub fn open_picker(&mut self) {
        self.view = IntakeView::Picker;
        self.completed = None;
        self.error = None;
        self.step_error = false;
        self.list_requested = false;
        self.loading_draft = false;
        self.popped_out = false;
        self.reset_relationships();
    }

    pub fn start(&mut self, kind: IntakeKind) {
        self.draft = Some(IntakeDraft::new(kind));
        self.view = IntakeView::Wizard;
        self.completed = None;
        self.error = None;
        self.step_error = false;
        self.saving = false;
        self.dirty = true;
        self.save_due = Some(Instant::now());
        self.popped_out = false;
        self.reset_relationships();
    }

    pub fn mark_edited(&mut self) {
        self.dirty = true;
        self.save_due = Some(Instant::now() + Duration::from_millis(700));
    }
}

impl Default for IntakeState {
    fn default() -> Self {
        Self::new()
    }
}

pub enum IntakeAction {
    Start(IntakeKind),
    Resume(String),
    Discard(String),
    Edited,
    SaveNow,
    BackToPicker,
    Complete,
    CreateRelationshipContact {
        slot: String,
        role: RoleOption,
        notes: String,
        draft: Box<NewRelationshipContact>,
    },
    ViewContact {
        filter: ContactFilter,
        name: String,
    },
    GoToSettings,
    PopOut,
    Dock,
}

pub fn show(
    ui: &mut Ui,
    state: &mut IntakeState,
    store_ready: bool,
    signed_in: bool,
    catalog: &[ApiGraphNode],
    roles: &[RoleOption],
) -> Option<IntakeAction> {
    if !signed_in {
        header(
            ui,
            "Intake",
            "Sign in to the server before starting an onboarding workflow.",
        );
        ui.add_space(16.0);
        let mut action = None;
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("Server sign-in required")
                    .font(theme::strong_font(16.0)),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Completed intakes are saved directly to the CareLibre server. Sign in with your organisation account to begin.",
                )
                .color(theme::muted())
                .size(12.0),
            );
            ui.add_space(10.0);
            if ui.add(theme::primary_button("Go to Settings")).clicked() {
                action = Some(IntakeAction::GoToSettings);
            }
        });
        return action;
    }

    if state.view == IntakeView::Wizard && state.popped_out {
        header(
            ui,
            "Intake",
            "This onboarding workflow is open in its own window.",
        );
        ui.add_space(16.0);
        let mut action = None;
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("Intake is open in a separate window").font(theme::strong_font(16.0)),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new("Continue the wizard in the native window, or bring it back here.")
                    .color(theme::muted())
                    .size(12.0),
            );
            ui.add_space(10.0);
            if ui.add(theme::primary_button("Dock intake")).clicked() {
                action = Some(IntakeAction::Dock);
            }
        });
        return action;
    }

    match state.view {
        IntakeView::Picker => {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| picker(ui, state, store_ready))
                .inner
        }
        IntakeView::Wizard => {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| wizard(ui, state, store_ready, catalog, roles))
                .inner
        }
        IntakeView::Completed => completed(ui, state),
    }
}

// Picker -------------------------------------------------------------------

fn picker(ui: &mut Ui, state: &mut IntakeState, store_ready: bool) -> Option<IntakeAction> {
    let mut action = None;

    header(
        ui,
        "Intake",
        "Start a guided onboarding workflow, or pick up a saved draft.",
    );
    ui.add_space(20.0);

    if !store_ready {
        message_card(
            ui,
            "Starting local cache",
            "Intake drafts will be available as soon as the cache opens.",
            theme::muted(),
        );
        return None;
    }

    if let Some(error) = &state.error {
        ui.add_space(6.0);
        message_card(ui, "Intake error", error, theme::danger());
        ui.add_space(10.0);
    }

    ui.columns(3, |cols| {
        for (index, kind) in [IntakeKind::Client, IntakeKind::Worker, IntakeKind::Other]
            .into_iter()
            .enumerate()
        {
            if scenario_card(&mut cols[index], kind).clicked() {
                action = Some(IntakeAction::Start(kind));
            }
        }
    });

    ui.add_space(20.0);
    section_header(ui, "Drafts", state.drafts.len());

    if state.list_loading {
        ui.label(
            RichText::new("Loading saved drafts…")
                .color(theme::faint())
                .size(11.0),
        );
    } else if state.drafts.is_empty() {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("No drafts yet")
                    .color(theme::muted())
                    .font(theme::strong_font(13.0)),
            );
            ui.label(
                RichText::new(
                    "Start a scenario above. Drafts save automatically and appear here \
                     if you close the wizard.",
                )
                .color(theme::faint())
                .size(11.0),
            );
        });
    } else {
        for draft in &state.drafts {
            let total = fields::steps(draft.kind).len();
            let response = Frame::new()
                .fill(theme::surface())
                .stroke(Stroke::new(1.0, theme::border()))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(9),
                            theme::surface_raised(),
                        );
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            draft.kind.icon(),
                            FontId::proportional(15.0),
                            theme::text(),
                        );
                        ui.add_space(2.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&draft.name).font(theme::strong_font(13.0)));
                            ui.horizontal(|ui| {
                                theme::pill(ui, draft.kind.short_label(), kind_colour(draft.kind));
                                ui.label(
                                    RichText::new(format!(
                                        "Step {} of {} · {}",
                                        draft.current_step + 1,
                                        total,
                                        relative_time(draft.updated_at)
                                    ))
                                    .color(theme::faint())
                                    .size(10.5),
                                );
                            });
                        });

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(theme::primary_button("Resume")).clicked() {
                                action = Some(IntakeAction::Resume(draft.id.clone()));
                            }
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Discard").color(theme::danger()).size(11.0),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                action = Some(IntakeAction::Discard(draft.id.clone()));
                            }
                        });
                    });
                });
            let _ = response;
            ui.add_space(6.0);
        }
    }

    ui.add_space(12.0);
    ui.label(
        RichText::new(
            "Drafts are stored in the local SQLite cache and never block on the network.",
        )
        .color(theme::faint())
        .size(10.0),
    );

    action
}

fn scenario_card(ui: &mut Ui, kind: IntakeKind) -> egui::Response {
    let inner = theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(150.0);

        let (rect, _) = ui.allocate_exact_size(vec2(42.0, 42.0), Sense::hover());
        ui.painter()
            .rect_filled(rect, CornerRadius::same(11), theme::surface_raised());
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            kind.icon(),
            FontId::proportional(20.0),
            theme::text(),
        );

        ui.add_space(8.0);
        ui.label(RichText::new(kind.short_label()).font(theme::strong_font(16.0)));
        ui.label(
            RichText::new(kind.description())
                .color(theme::muted())
                .size(11.5),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!("{} steps  →", fields::steps(kind).len()))
                .color(theme::accent())
                .size(11.0),
        );
    });

    let response = inner.response.interact(Sense::click());
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            ui.is_enabled(),
            format!("Start {}", kind.label()),
        )
    });
    response
}

// Wizard -------------------------------------------------------------------

pub fn show_wizard_viewport(
    ui: &mut Ui,
    state: &mut IntakeState,
    client_side_decorations: bool,
    catalog: &[ApiGraphNode],
    roles: &[RoleOption],
) -> Option<IntakeAction> {
    if state.view != IntakeView::Wizard {
        return Some(IntakeAction::Dock);
    }
    if ui.ctx().input(|i| i.viewport().close_requested()) {
        return Some(IntakeAction::Dock);
    }

    if client_side_decorations {
        let title = state
            .draft
            .as_ref()
            .map(|draft| format!("Intake — {}", draft.display_name()))
            .unwrap_or_else(|| "Intake — CareLibre".to_owned());
        let ctx = ui.ctx().clone();
        egui::Panel::top("intake-window-chrome")
            .exact_size(crate::chrome::HEIGHT)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::sidebar())
                    .inner_margin(Margin::ZERO),
            )
            .show(ui, |ui| crate::chrome::title_bar(ui, &ctx, &title));
    }

    let mut action = None;
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(theme::bg())
                .inner_margin(Margin::same(16)),
        )
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    action = wizard(ui, state, true, catalog, roles);
                });
        });

    if client_side_decorations {
        crate::chrome::resize_handles(ui.ctx());
    }

    action
}

fn wizard(
    ui: &mut Ui,
    state: &mut IntakeState,
    store_ready: bool,
    catalog: &[ApiGraphNode],
    roles: &[RoleOption],
) -> Option<IntakeAction> {
    if !store_ready {
        message_card(
            ui,
            "Starting local cache",
            "The intake wizard needs the local cache. It will be ready in a moment.",
            theme::muted(),
        );
        return None;
    }

    if state.loading_draft {
        message_card(
            ui,
            "Opening draft",
            "Loading the saved intake from the local cache.",
            theme::muted(),
        );
        return None;
    }

    let Some(mut draft) = state.draft.clone() else {
        message_card(
            ui,
            "No draft selected",
            "Choose an intake scenario to begin.",
            theme::muted(),
        );
        return None;
    };

    let steps = fields::steps(draft.kind);
    let step_index = draft.current_step.min(steps.len().saturating_sub(1));
    let step = &steps[step_index];
    let mut action = None;

    // Header
    ui.horizontal(|ui| {
        if ui
            .add(
                egui::Button::new(RichText::new("←  Intake").color(theme::muted()).size(11.5))
                    .frame(false),
            )
            .clicked()
        {
            action = Some(IntakeAction::BackToPicker);
        }
        ui.add_space(6.0);
        ui.label(RichText::new(draft.display_name()).font(theme::strong_font(22.0)));
        theme::pill(ui, draft.kind.short_label(), kind_colour(draft.kind));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (label, next) = if state.popped_out {
                ("Dock", IntakeAction::Dock)
            } else {
                ("Pop out", IntakeAction::PopOut)
            };
            if ui
                .add(
                    egui::Button::new(RichText::new(label).color(theme::accent()).size(11.0))
                        .frame(false),
                )
                .clicked()
            {
                action = Some(next);
            }

            let (text, colour) = if state.saving {
                ("Saving…".to_owned(), theme::warning())
            } else if state.dirty {
                ("Unsaved changes".to_owned(), theme::warning())
            } else {
                ("Saved locally".to_owned(), theme::success())
            };
            ui.label(RichText::new(text).color(colour).size(10.5));
        });
    });

    ui.add_space(10.0);
    progress(ui, steps, step_index);
    ui.add_space(10.0);

    // Step chips
    let mut clicked_step = None;
    ui.horizontal_wrapped(|ui| {
        for (index, item) in steps.iter().enumerate() {
            let selected = index == step_index;
            let visited = index <= step_index;
            let label = format!("{}  {}", index + 1, item.title);
            let button = egui::Button::new(
                RichText::new(label)
                    .color(if selected {
                        theme::on_accent()
                    } else if visited {
                        theme::text()
                    } else {
                        theme::faint()
                    })
                    .font(theme::strong_font(10.0)),
            )
            .fill(if selected {
                theme::accent()
            } else if visited {
                theme::accent_soft(22)
            } else {
                Color32::TRANSPARENT
            })
            .corner_radius(CornerRadius::same(7));

            let response = ui.add_enabled(visited, button);
            if response.clicked() && index != step_index {
                clicked_step = Some(index);
            }
        }
    });

    if let Some(index) = clicked_step {
        draft.current_step = index;
        state.step_error = false;
        state.error = None;
        state.mark_edited();
        action = Some(IntakeAction::Edited);
    }

    ui.add_space(14.0);

    let width = ui.available_width();
    let gap = 18.0;
    let right_width = 280.0f32.min(width * 0.30);
    let left_width = (width - right_width - gap).max(420.0);

    let mut changed = false;

    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(vec2(left_width, 0.0), Layout::top_down(Align::Min), |ui| {
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());

                ui.label(RichText::new(step.title).font(theme::strong_font(18.0)));
                ui.label(
                    RichText::new(step.description)
                        .color(theme::muted())
                        .size(12.0),
                );
                ui.add_space(14.0);

                changed |= render_fields(ui, &mut draft, step, state.step_error);
                let (relationship_changed, relationship_action) =
                    render_relationships(ui, &mut draft, step, state, catalog, roles);
                changed |= relationship_changed;
                if relationship_action.is_some() {
                    action = relationship_action;
                }

                if let Some(error) = &state.error {
                    ui.add_space(8.0);
                    message_inline(ui, error, theme::danger());
                }

                ui.add_space(14.0);
                ui.separator();
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    if ui.add(theme::secondary_button("Back")).clicked() {
                        if step_index == 0 {
                            action = Some(IntakeAction::BackToPicker);
                        } else {
                            draft.current_step -= 1;
                            state.step_error = false;
                            state.error = None;
                            state.mark_edited();
                            action = Some(IntakeAction::Edited);
                        }
                    }

                    if ui.add(theme::secondary_button("Save & close")).clicked() {
                        action = Some(IntakeAction::BackToPicker);
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let last = step_index + 1 == steps.len();
                        let label = if last { "Finish intake" } else { "Continue" };
                        if ui
                            .add_enabled(!state.relationship_pending, theme::primary_button(label))
                            .clicked()
                        {
                            if last {
                                if let Some((index, error)) = validate_all(&draft) {
                                    draft.current_step = index;
                                    state.error = Some(error);
                                    state.step_error = true;
                                    state.mark_edited();
                                } else {
                                    action = Some(IntakeAction::Complete);
                                }
                            } else if let Some(error) = validate(step, &draft) {
                                state.error = Some(error);
                                state.step_error = true;
                            } else {
                                draft.current_step += 1;
                                state.step_error = false;
                                state.error = None;
                                state.mark_edited();
                                action = Some(IntakeAction::Edited);
                            }
                        }
                        if state.relationship_pending {
                            ui.label(
                                RichText::new("Creating contact…")
                                    .color(theme::warning())
                                    .size(10.5),
                            );
                        }
                    });
                });
            });
        });

        ui.add_space(gap);

        ui.allocate_ui_with_layout(vec2(right_width, 0.0), Layout::top_down(Align::Min), |ui| {
            summary_card(ui, state, &draft, steps.len(), step_index);
        });
    });

    if changed {
        state.mark_edited();
        if action.is_none() {
            action = Some(IntakeAction::Edited);
        }
    }

    state.draft = Some(draft);
    action
}

fn summary_card(
    ui: &mut Ui,
    state: &IntakeState,
    draft: &IntakeDraft,
    total_steps: usize,
    step_index: usize,
) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Intake summary").font(theme::strong_font(13.5)));
        ui.add_space(8.0);

        ui.label(RichText::new(draft.display_name()).font(theme::strong_font(12.5)));
        ui.label(
            RichText::new(draft.kind.label())
                .color(theme::muted())
                .size(11.0),
        );
        ui.add_space(8.0);

        ui.label(
            RichText::new(format!("Step {} of {}", step_index + 1, total_steps))
                .color(theme::text())
                .size(11.5),
        );

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(8.0);

        let (label, colour) = if state.saving {
            ("Saving draft…", theme::warning())
        } else if state.dirty {
            ("Autosave pending", theme::warning())
        } else {
            ("Draft saved", theme::success())
        };
        ui.label(RichText::new(label).color(colour).size(10.5));
        ui.label(
            RichText::new(
                "Drafts are stored locally; completing the intake creates the contact on the server.",
            )
            .color(theme::faint())
            .size(9.5),
        );

        ui.add_space(12.0);
        ui.label(
            RichText::new("Tips")
                .color(theme::muted())
                .font(theme::strong_font(10.5)),
        );
        for tip in [
            "Only fields marked * are required.",
            "Dates use DD/MM/YYYY.",
            "You can save and close at any step.",
            "Resume from the intake picker.",
        ] {
            ui.label(
                RichText::new(format!("· {tip}"))
                    .color(theme::faint())
                    .size(9.5),
            );
        }
    });
}

fn render_fields(ui: &mut Ui, draft: &mut IntakeDraft, step: &StepSpec, show_errors: bool) -> bool {
    let mut changed = false;
    let mut pending: Option<&FieldSpec> = None;

    for field in step.fields {
        if field.wide {
            if let Some(first) = pending.take() {
                changed |= render_field(ui, draft, first, show_errors);
            }
            changed |= render_field(ui, draft, field, show_errors);
            continue;
        }

        if let Some(first) = pending.take() {
            let mut pair_changed = false;
            ui.columns(2, |cols| {
                if render_field(&mut cols[0], draft, first, show_errors) {
                    pair_changed = true;
                }
                if render_field(&mut cols[1], draft, field, show_errors) {
                    pair_changed = true;
                }
            });
            changed |= pair_changed;
        } else {
            pending = Some(field);
        }
    }

    if let Some(field) = pending {
        changed |= render_field(ui, draft, field, show_errors);
    }

    changed
}

fn render_relationships(
    ui: &mut Ui,
    draft: &mut IntakeDraft,
    step: &StepSpec,
    state: &mut IntakeState,
    catalog: &[ApiGraphNode],
    roles: &[RoleOption],
) -> (bool, Option<IntakeAction>) {
    if step.relationships.is_empty() {
        return (false, None);
    }

    let mut changed = false;
    let mut action = None;

    ui.add_space(4.0);
    ui.separator();
    ui.add_space(8.0);
    ui.label(RichText::new("Linked contacts").font(theme::strong_font(13.0)));
    ui.label(
        RichText::new(
            "Search the directory or create a new contact. Relationships are linked when the intake is completed.",
        )
        .color(theme::muted())
        .size(10.5),
    );

    if let Some(error) = &state.relationship_error {
        ui.add_space(6.0);
        message_inline(ui, error, theme::danger());
    }
    ui.add_space(8.0);

    for spec in step.relationships {
        let current = draft.relationship(spec.key).cloned();
        let invalid = state.step_error && spec.required && current.is_none();
        relationship_label(ui, spec, invalid);

        if let Some(current) = current {
            Frame::new()
                .fill(theme::surface_raised())
                .stroke(Stroke::new(1.0, theme::border()))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(relationship_role_label(&current.linked_role))
                                .font(theme::strong_font(12.0)),
                        );
                        theme::pill(ui, current.kind.label(), theme::accent_blue());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Remove").color(theme::danger()).size(10.5),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                draft.remove_relationship(spec.key);
                                state.relationship_forms.remove(spec.key);
                                changed = true;
                            }
                        });
                    });
                    ui.label(RichText::new(&current.name).color(theme::text()).size(12.0));
                    if !current.notes.trim().is_empty() {
                        ui.label(
                            RichText::new(&current.notes)
                                .color(theme::muted())
                                .size(10.5),
                        );
                    }
                });
            ui.add_space(6.0);
            continue;
        }

        let mut open_new = None;
        {
            let form = state
                .relationship_forms
                .entry(spec.key.to_owned())
                .or_insert_with(|| IntakeRelationshipForm {
                    role: preferred_role(spec, roles),
                    target: None,
                    notes: String::new(),
                });
            if form.role.is_none() {
                form.role = preferred_role(spec, roles);
            }

            if let Some(role) = form.role.clone() {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} · this record is {}",
                            relationship_role_label(&role.linked_role),
                            relationship_role_label(&role.subject_role)
                        ))
                        .color(theme::faint())
                        .size(10.0),
                    );
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Change role")
                                    .color(theme::accent())
                                    .size(10.0),
                            )
                            .frame(false),
                        )
                        .clicked()
                    {
                        form.role = None;
                    }
                });
            } else {
                role_picker(
                    ui,
                    &format!("intake-role-{}", spec.key),
                    roles,
                    &mut form.role,
                    RoleOption::describe,
                );
            }

            party_lookup(
                ui,
                &format!("intake-party-{}", spec.key),
                &mut form.target,
                None,
                catalog,
            );

            ui.add(
                egui::TextEdit::singleline(&mut form.notes)
                    .hint_text("Relationship or notes (optional)")
                    .desired_width(ui.available_width())
                    .margin(Margin::symmetric(8, 6)),
            );

            ui.horizontal(|ui| {
                let can_add =
                    form.target.is_some() && form.role.is_some() && !state.relationship_pending;
                if ui
                    .add_enabled(can_add, theme::secondary_button("Add relationship"))
                    .clicked()
                    && let (Some(target), Some(role)) = (form.target.take(), form.role.clone())
                {
                    draft.set_relationship(
                        spec.key,
                        IntakeRelationship {
                            subject_role: role.subject_role,
                            linked_role: role.linked_role,
                            kind: target.kind,
                            id: target.id,
                            name: target.label,
                            notes: form.notes.trim().to_owned(),
                        },
                    );
                    form.notes.clear();
                    changed = true;
                }

                if ui
                    .add_enabled(
                        form.role.is_some() && !state.relationship_pending,
                        theme::secondary_button("+ New contact"),
                    )
                    .clicked()
                    && let Some(role) = form.role.clone()
                {
                    open_new = Some(IntakeNewRelationship {
                        slot: spec.key.to_owned(),
                        role,
                        notes: form.notes.trim().to_owned(),
                        contact: NewRelationshipContact::new(),
                    });
                }
            });
        }

        if let Some(new_rel) = open_new {
            state.new_relationship = Some(new_rel);
        }

        if state
            .new_relationship
            .as_ref()
            .is_some_and(|new_rel| new_rel.slot == spec.key)
            && let Some(next) = new_relationship_form(ui, state, spec)
        {
            action = Some(next);
        }

        ui.add_space(6.0);
    }

    (changed, action)
}

fn preferred_role(spec: &RelationshipSpec, roles: &[RoleOption]) -> Option<RoleOption> {
    roles
        .iter()
        .find(|role| role.subject_role == spec.subject_role && role.linked_role == spec.linked_role)
        .or_else(|| {
            roles
                .iter()
                .find(|role| role.linked_role == spec.linked_role)
        })
        .cloned()
}

fn relationship_label(ui: &mut Ui, spec: &RelationshipSpec, invalid: bool) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(spec.label)
                .color(if invalid {
                    theme::danger()
                } else {
                    theme::muted()
                })
                .size(10.5),
        );
        if spec.required {
            ui.label(
                RichText::new("*")
                    .color(if invalid {
                        theme::danger()
                    } else {
                        theme::accent()
                    })
                    .size(10.5),
            );
        }
    });
    ui.add_space(3.0);
}

fn new_relationship_form(
    ui: &mut Ui,
    state: &mut IntakeState,
    spec: &RelationshipSpec,
) -> Option<IntakeAction> {
    let mut new_rel = state.new_relationship.take()?;
    if new_rel.slot != spec.key {
        state.new_relationship = Some(new_rel);
        return None;
    }

    let pending = state.relationship_pending;
    let error = state.relationship_error.clone();
    let mut action = None;
    let mut cancelled = false;

    ui.add_space(6.0);
    Frame::new()
        .fill(theme::input())
        .stroke(Stroke::new(1.0, theme::accent().gamma_multiply(0.5)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(format!("New contact for {}", spec.label))
                    .font(theme::strong_font(12.5))
                    .color(theme::text()),
            );
            ui.add_space(4.0);
            ui.columns(2, |cols| {
                text_field(&mut cols[0], "Given name", &mut new_rel.contact.given_name);
                text_field(
                    &mut cols[1],
                    "Family name",
                    &mut new_rel.contact.family_name,
                );
            });
            ui.columns(2, |cols| {
                text_field(&mut cols[0], "Email", &mut new_rel.contact.email);
                text_field(&mut cols[1], "Phone", &mut new_rel.contact.phone);
            });

            if let Some(problem) = new_contact_error(&new_rel.contact) {
                ui.label(RichText::new(problem).color(theme::danger()).size(10.0));
            } else if let Some(error) = &error {
                ui.label(RichText::new(error).color(theme::danger()).size(10.0));
            }

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let valid = new_contact_error(&new_rel.contact).is_none();
                if ui
                    .add_enabled(valid && !pending, theme::primary_button("Create & link"))
                    .clicked()
                {
                    state.relationship_pending = true;
                    state.relationship_error = None;
                    action = Some(IntakeAction::CreateRelationshipContact {
                        slot: new_rel.slot.clone(),
                        role: new_rel.role.clone(),
                        notes: new_rel.notes.clone(),
                        draft: Box::new(new_rel.contact.clone()),
                    });
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    cancelled = true;
                    state.relationship_error = None;
                }
                if pending {
                    ui.label(
                        RichText::new("Creating contact…")
                            .color(theme::warning())
                            .size(10.0),
                    );
                }
            });
        });

    if !cancelled {
        state.new_relationship = Some(new_rel);
    }
    action
}

fn new_contact_error(contact: &NewRelationshipContact) -> Option<&'static str> {
    if contact.given_name.trim().is_empty() && contact.family_name.trim().is_empty() {
        return Some("A given or family name is required.");
    }
    if !contact.email.trim().is_empty() && !valid_email(contact.email.trim()) {
        return Some("Enter a valid email address.");
    }
    if !contact.phone.trim().is_empty() && !valid_phone(contact.phone.trim()) {
        return Some("Enter a valid phone number.");
    }
    None
}

fn render_field(
    ui: &mut Ui,
    draft: &mut IntakeDraft,
    field: &FieldSpec,
    show_errors: bool,
) -> bool {
    let value = draft.get(field.key).to_owned();
    let error = show_errors.then(|| field_error(draft, field)).flatten();
    let invalid = error.is_some();

    if field.kind == FieldKind::Checkbox {
        let mut checked = value == "true";
        let response = ui.horizontal(|ui| {
            let response = ui.checkbox(&mut checked, RichText::new(field.label).size(11.5));
            if field.required {
                ui.label(
                    RichText::new("*")
                        .color(if invalid {
                            theme::danger()
                        } else {
                            theme::accent()
                        })
                        .size(11.0),
                );
            }
            response
        });
        let changed = response.inner.changed();
        if changed {
            draft.set(field.key, checked.to_string());
        }
        if let Some(error) = &error {
            ui.label(RichText::new(error).color(theme::danger()).size(10.0));
        }
        ui.add_space(6.0);
        return changed;
    }

    field_label(ui, field, invalid);

    let changed = match field.kind {
        FieldKind::Multiline => {
            let mut text = value.clone();
            let response = ui.add(
                egui::TextEdit::multiline(&mut text)
                    .hint_text(field.placeholder)
                    .desired_rows(3)
                    .desired_width(ui.available_width())
                    .margin(Margin::symmetric(10, 8)),
            );
            if response.changed() {
                draft.set(field.key, text);
            }
            response.changed()
        }
        FieldKind::Select => {
            let mut current = value.clone();
            let previous = current.clone();
            let id = format!("intake-{}-{}", draft.id, field.key);
            let response = egui::ComboBox::from_id_salt(id)
                .selected_text(if current.is_empty() {
                    field.placeholder
                } else {
                    &current
                })
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for option in field.options {
                        ui.selectable_value(&mut current, (*option).to_owned(), *option);
                    }
                });
            response.response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::ComboBox, ui.is_enabled(), field.label)
            });
            if current != previous {
                draft.set(field.key, current);
                true
            } else {
                false
            }
        }
        _ => {
            let mut text = value.clone();
            let mut edit = egui::TextEdit::singleline(&mut text)
                .hint_text(field.placeholder)
                .desired_width(ui.available_width())
                .margin(Margin::symmetric(10, 8));
            if field.kind == FieldKind::Email {
                edit = edit.hint_text(if field.placeholder.is_empty() {
                    "name@example.com"
                } else {
                    field.placeholder
                });
            }
            let response = ui.add(edit);
            if response.changed() {
                draft.set(field.key, text);
            }
            response.changed()
        }
    };

    if let Some(error) = &error {
        ui.label(RichText::new(error).color(theme::danger()).size(10.0));
    }

    ui.add_space(6.0);
    changed
}

fn text_field(ui: &mut Ui, label: &str, text: &mut String) {
    ui.label(RichText::new(label).color(theme::muted()).size(10.5));
    ui.add(
        egui::TextEdit::singleline(text)
            .desired_width(ui.available_width())
            .margin(Margin::symmetric(8, 6)),
    );
    ui.add_space(4.0);
}

fn field_label(ui: &mut Ui, field: &FieldSpec, invalid: bool) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(field.label)
                .color(if invalid {
                    theme::danger()
                } else {
                    theme::muted()
                })
                .size(10.5),
        );
        if field.required {
            ui.label(
                RichText::new("*")
                    .color(if invalid {
                        theme::danger()
                    } else {
                        theme::accent()
                    })
                    .size(10.5),
            );
        }
    });
    ui.add_space(3.0);
}

fn validate(step: &StepSpec, draft: &IntakeDraft) -> Option<String> {
    let mut errors = Vec::new();

    for field in step.fields {
        if let Some(error) = field_error(draft, field) {
            errors.push(format!("{}: {error}", field.label));
        }
    }

    for relationship in step.relationships {
        if relationship.required && draft.relationship(relationship.key).is_none() {
            errors.push(format!(
                "{}: choose a contact or create a new one.",
                relationship.label
            ));
        }
    }

    errors.extend(cross_field_errors(draft));

    if errors.is_empty() {
        None
    } else {
        Some(errors.join(" · "))
    }
}

fn validate_all(draft: &IntakeDraft) -> Option<(usize, String)> {
    for (index, step) in fields::steps(draft.kind).iter().enumerate() {
        if let Some(error) = validate(step, draft) {
            return Some((index, error));
        }
    }
    None
}

fn field_error(draft: &IntakeDraft, field: &FieldSpec) -> Option<String> {
    if field.kind == FieldKind::Checkbox {
        return (field.required && !draft.is_checked(field.key)).then(|| "Required.".to_owned());
    }

    let value = draft.get(field.key).trim();
    if value.is_empty() {
        return field.required.then(|| "Required.".to_owned());
    }

    match field.kind {
        FieldKind::Email if !valid_email(value) => Some("Enter a valid email address.".to_owned()),
        FieldKind::Phone if !valid_phone(value) => Some("Enter a valid phone number.".to_owned()),
        FieldKind::Date if parse_date(value).is_none() => Some("Use DD/MM/YYYY.".to_owned()),
        FieldKind::Number => match value.parse::<f64>() {
            Err(_) => Some("Enter a number.".to_owned()),
            Ok(number) if !number.is_finite() => Some("Enter a finite number.".to_owned()),
            Ok(number) if number < 0.0 => Some("Enter a positive number.".to_owned()),
            Ok(number) if field.key == "superannuation" && number > 100.0 => {
                Some("Enter a percentage between 0 and 100.".to_owned())
            }
            Ok(_) => None,
        },
        FieldKind::Postcode if value.len() != 4 || !value.chars().all(|ch| ch.is_ascii_digit()) => {
            Some("Enter a 4-digit postcode.".to_owned())
        }
        FieldKind::Select if !field.options.contains(&value) => {
            Some("Choose one of the listed options.".to_owned())
        }
        _ => None,
    }
}

fn cross_field_errors(draft: &IntakeDraft) -> Vec<String> {
    let mut errors = Vec::new();

    if let Some(date_of_birth) = parse_date(draft.get("date_of_birth"))
        && date_of_birth > chrono::Local::now().date_naive()
    {
        errors.push("Date of birth cannot be in the future.".to_owned());
    }

    date_order(
        draft,
        "start_date",
        "end_date",
        "Contract end must be on or after contract start.",
        &mut errors,
    );
    date_order(
        draft,
        "plan_start",
        "plan_end",
        "Plan end must be on or after plan start.",
        &mut errors,
    );

    errors
}

fn date_order(
    draft: &IntakeDraft,
    start_key: &str,
    end_key: &str,
    message: &str,
    errors: &mut Vec<String>,
) {
    if let (Some(start), Some(end)) = (
        parse_date(draft.get(start_key)),
        parse_date(draft.get(end_key)),
    ) && end < start
    {
        errors.push(message.to_owned());
    }
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value.trim(), "%d/%m/%Y").ok()
}

fn valid_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !value.chars().any(char::is_whitespace)
        && !domain.contains('@')
}

fn valid_phone(value: &str) -> bool {
    let mut digits = 0;
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            digits += 1;
        } else if !matches!(ch, ' ' | '+' | '-' | '(' | ')' | '.') {
            return false;
        }
    }
    (8..=15).contains(&digits)
}

// Completed ----------------------------------------------------------------

fn completed(ui: &mut Ui, state: &mut IntakeState) -> Option<IntakeAction> {
    let Some(completed) = &state.completed else {
        state.open_picker();
        return Some(IntakeAction::BackToPicker);
    };
    let mut action = None;

    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(58.0, 58.0), Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 28.0, theme::accent_soft(40));
        ui.painter()
            .circle_stroke(rect.center(), 27.0, Stroke::new(1.0, theme::accent()));
        let tick = Stroke::new(2.0, theme::accent());
        ui.painter().line_segment(
            [
                rect.center() + vec2(-8.0, 0.0),
                rect.center() + vec2(-2.0, 6.0),
            ],
            tick,
        );
        ui.painter().line_segment(
            [
                rect.center() + vec2(-2.0, 6.0),
                rect.center() + vec2(9.0, -7.0),
            ],
            tick,
        );

        ui.add_space(14.0);
        ui.label(RichText::new("Intake complete").font(theme::strong_font(26.0)));
        ui.label(
            RichText::new(format!("{} has been added to the local cache.", completed.name))
                .color(theme::muted())
                .size(13.0),
        );
        ui.add_space(4.0);
        theme::pill(ui, completed.kind.short_label(), kind_colour(completed.kind));
        ui.add_space(4.0);
        ui.label(
            RichText::new("The contact is saved with status Onboarding and will sync when the server API is available.")
                .color(theme::faint())
                .size(10.5),
        );

        ui.add_space(20.0);
        ui.horizontal(|ui| {
            if ui.add(theme::primary_button("View contact")).clicked() {
                let filter = match completed.kind {
                    IntakeKind::Client => ContactFilter::Clients,
                    IntakeKind::Worker => ContactFilter::Workers,
                    IntakeKind::Other => ContactFilter::Other,
                };
                action = Some(IntakeAction::ViewContact {
                    filter,
                    name: completed.name.clone(),
                });
            }
            if ui.add(theme::secondary_button("Start another intake")).clicked() {
                action = Some(IntakeAction::BackToPicker);
            }
        });
    });

    action
}

// Shared -------------------------------------------------------------------

fn header(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(52.0, 52.0), Sense::hover());
        ui.painter()
            .rect_filled(rect, CornerRadius::same(14), theme::surface_raised());
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "📝",
            FontId::proportional(22.0),
            theme::text(),
        );
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(title).font(theme::strong_font(26.0)));
            ui.label(RichText::new(subtitle).color(theme::muted()).size(13.0));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            theme::pill(ui, "WIZARDS", theme::accent());
        });
    });
}

fn section_header(ui: &mut Ui, title: &str, count: usize) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(theme::strong_font(15.0)));
        theme::pill(ui, &count.to_string(), theme::faint());
    });
    ui.add_space(8.0);
}

fn progress(ui: &mut Ui, steps: &[StepSpec], current: usize) {
    let fraction = (current + 1) as f32 / steps.len().max(1) as f32;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 5.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(3), theme::surface_raised());
    let filled = egui::Rect::from_min_size(rect.min, vec2(rect.width() * fraction, 5.0));
    ui.painter()
        .rect_filled(filled, CornerRadius::same(3), theme::accent());
}

fn message_card(ui: &mut Ui, title: &str, message: &str, colour: Color32) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(title)
                .color(colour)
                .font(theme::strong_font(14.0)),
        );
        ui.add_space(4.0);
        ui.label(RichText::new(message).color(theme::muted()).size(12.0));
    });
}

fn message_inline(ui: &mut Ui, message: &str, colour: Color32) {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(
            colour.r(),
            colour.g(),
            colour.b(),
            20,
        ))
        .stroke(Stroke::new(1.0, colour.gamma_multiply(0.55)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(message).color(colour).size(11.0));
        });
}

fn kind_colour(kind: IntakeKind) -> Color32 {
    match kind {
        IntakeKind::Client => theme::accent_blue(),
        IntakeKind::Worker => theme::accent(),
        IntakeKind::Other => theme::warning(),
    }
}

fn relative_time(updated_at: i64) -> String {
    let now = crate::intake::now_ms();
    let seconds = ((now - updated_at).max(0) / 1000) as u64;
    match seconds {
        0..=59 => "just now".to_owned(),
        60..=3599 => format!("{}m ago", seconds / 60),
        3600..=86_399 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact::PartyKind;
    use crate::intake::IntakeKind;

    fn field(key: &str) -> &'static FieldSpec {
        fields::CLIENT_STEPS
            .iter()
            .flat_map(|step| step.fields)
            .find(|field| field.key == key)
            .expect("field exists")
    }

    #[test]
    fn validates_common_formats() {
        assert!(valid_email("owner@example.com"));
        assert!(!valid_email("owner@example"));
        assert!(!valid_email("owner example.com"));

        assert!(valid_phone("0400 000 000"));
        assert!(valid_phone("+61 400 000 000"));
        assert!(!valid_phone("call me"));
        assert!(!valid_phone("123"));
    }

    #[test]
    fn reports_invalid_field_values() {
        let mut draft = IntakeDraft::new(IntakeKind::Client);
        draft.set("email", "not-an-email".to_owned());
        assert!(field_error(&draft, field("email")).is_some());

        draft.set("email", "owner@example.com".to_owned());
        assert!(field_error(&draft, field("email")).is_none());

        draft.set("date_of_birth", "31/02/2020".to_owned());
        assert!(field_error(&draft, field("date_of_birth")).is_some());

        draft.set("postcode", "200".to_owned());
        assert!(field_error(&draft, field("postcode")).is_some());
        draft.set("postcode", "2000".to_owned());
        assert!(field_error(&draft, field("postcode")).is_none());
    }

    #[test]
    fn requires_the_emergency_contact_relationship() {
        let mut draft = IntakeDraft::new(IntakeKind::Client);
        let step = &fields::CLIENT_STEPS[3];

        let error = validate(step, &draft).expect("missing emergency contact");
        assert!(error.contains("Emergency contact"));

        draft.set_relationship(
            "emergency_contact",
            IntakeRelationship {
                subject_role: "person_supported".to_owned(),
                linked_role: "emergency_contact".to_owned(),
                kind: PartyKind::Contact,
                id: "contact-1".to_owned(),
                name: "Alex Chen".to_owned(),
                notes: String::new(),
            },
        );
        assert!(validate(step, &draft).is_none());
    }

    #[test]
    fn rejects_future_dates_of_birth() {
        let mut draft = IntakeDraft::new(IntakeKind::Client);
        draft.set("first_name", "Margaret".to_owned());
        draft.set("last_name", "Chen".to_owned());
        draft.set("date_of_birth", "01/01/2999".to_owned());

        let error = validate(&fields::CLIENT_STEPS[0], &draft).expect("future date");
        assert!(error.contains("future"));
    }
}
