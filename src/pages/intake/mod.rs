//! Intake picker and guided onboarding wizards.

pub(crate) mod fields;

use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, RichText, Sense,
    Stroke, Ui, vec2,
};

use crate::intake::{CompletedIntake, IntakeDraft, IntakeKind, IntakeSummary};
use crate::pages::Page;
use crate::theme;

use fields::{FieldKind, FieldSpec, StepSpec};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IntakeView {
    Picker,
    Wizard,
    Completed,
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
        }
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
    ViewContact { page: Page, name: String },
    GoToSettings,
}

pub fn show(
    ui: &mut Ui,
    state: &mut IntakeState,
    store_ready: bool,
    signed_in: bool,
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
                .color(theme::MUTED)
                .size(12.0),
            );
            ui.add_space(10.0);
            if ui.add(theme::primary_button("Go to Settings")).clicked() {
                action = Some(IntakeAction::GoToSettings);
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
                .show(ui, |ui| wizard(ui, state, store_ready))
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
            theme::MUTED,
        );
        return None;
    }

    if let Some(error) = &state.error {
        ui.add_space(6.0);
        message_card(ui, "Intake error", error, theme::DANGER);
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
                .color(theme::FAINT)
                .size(11.0),
        );
    } else if state.drafts.is_empty() {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("No drafts yet")
                    .color(theme::MUTED)
                    .font(theme::strong_font(13.0)),
            );
            ui.label(
                RichText::new(
                    "Start a scenario above. Drafts save automatically and appear here \
                     if you close the wizard.",
                )
                .color(theme::FAINT)
                .size(11.0),
            );
        });
    } else {
        for draft in &state.drafts {
            let total = fields::steps(draft.kind).len();
            let response = Frame::new()
                .fill(theme::SURFACE)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(9),
                            theme::SURFACE_RAISED,
                        );
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            draft.kind.icon(),
                            FontId::proportional(15.0),
                            theme::TEXT,
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
                                    .color(theme::FAINT)
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
                                        RichText::new("Discard").color(theme::DANGER).size(11.0),
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
        .color(theme::FAINT)
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
            .rect_filled(rect, CornerRadius::same(11), theme::SURFACE_RAISED);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            kind.icon(),
            FontId::proportional(20.0),
            theme::TEXT,
        );

        ui.add_space(8.0);
        ui.label(RichText::new(kind.short_label()).font(theme::strong_font(16.0)));
        ui.label(
            RichText::new(kind.description())
                .color(theme::MUTED)
                .size(11.5),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!("{} steps  →", fields::steps(kind).len()))
                .color(theme::ACCENT)
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

fn wizard(ui: &mut Ui, state: &mut IntakeState, store_ready: bool) -> Option<IntakeAction> {
    if !store_ready {
        message_card(
            ui,
            "Starting local cache",
            "The intake wizard needs the local cache. It will be ready in a moment.",
            theme::MUTED,
        );
        return None;
    }

    if state.loading_draft {
        message_card(
            ui,
            "Opening draft",
            "Loading the saved intake from the local cache.",
            theme::MUTED,
        );
        return None;
    }

    let Some(mut draft) = state.draft.clone() else {
        message_card(
            ui,
            "No draft selected",
            "Choose an intake scenario to begin.",
            theme::MUTED,
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
                egui::Button::new(RichText::new("←  Intake").color(theme::MUTED).size(11.5))
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
            let (text, colour) = if state.saving {
                ("Saving…".to_owned(), theme::WARNING)
            } else if state.dirty {
                ("Unsaved changes".to_owned(), theme::WARNING)
            } else {
                ("Saved locally".to_owned(), theme::SUCCESS)
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
                        theme::BG
                    } else if visited {
                        theme::TEXT
                    } else {
                        theme::FAINT
                    })
                    .font(theme::strong_font(10.0)),
            )
            .fill(if selected {
                theme::ACCENT
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
                        .color(theme::MUTED)
                        .size(12.0),
                );
                ui.add_space(14.0);

                changed |= render_fields(ui, &mut draft, step, state.step_error);

                if let Some(error) = &state.error {
                    ui.add_space(8.0);
                    message_inline(ui, error, theme::DANGER);
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
                        if ui.add(theme::primary_button(label)).clicked() {
                            if let Some(error) = validate(step, &draft) {
                                state.error = Some(error);
                                state.step_error = true;
                            } else if last {
                                action = Some(IntakeAction::Complete);
                            } else {
                                draft.current_step += 1;
                                state.step_error = false;
                                state.error = None;
                                state.mark_edited();
                                action = Some(IntakeAction::Edited);
                            }
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
        if !matches!(action, Some(IntakeAction::Complete)) {
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
                .color(theme::MUTED)
                .size(11.0),
        );
        ui.add_space(8.0);

        ui.label(
            RichText::new(format!("Step {} of {}", step_index + 1, total_steps))
                .color(theme::TEXT)
                .size(11.5),
        );

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(8.0);

        let (label, colour) = if state.saving {
            ("Saving draft…", theme::WARNING)
        } else if state.dirty {
            ("Autosave pending", theme::WARNING)
        } else {
            ("Draft saved", theme::SUCCESS)
        };
        ui.label(RichText::new(label).color(colour).size(10.5));
        ui.label(
            RichText::new(
                "Drafts are stored locally; completing the intake creates the contact on the server.",
            )
            .color(theme::FAINT)
            .size(9.5),
        );

        ui.add_space(12.0);
        ui.label(
            RichText::new("Tips")
                .color(theme::MUTED)
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
                    .color(theme::FAINT)
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

fn render_field(
    ui: &mut Ui,
    draft: &mut IntakeDraft,
    field: &FieldSpec,
    show_errors: bool,
) -> bool {
    let value = draft.get(field.key).to_owned();
    let invalid = show_errors && field.required && !field_filled(draft, field);

    if field.kind == FieldKind::Checkbox {
        let mut checked = value == "true";
        let response = ui.horizontal(|ui| {
            let response = ui.checkbox(&mut checked, RichText::new(field.label).size(11.5));
            if field.required {
                ui.label(
                    RichText::new("*")
                        .color(if invalid {
                            theme::DANGER
                        } else {
                            theme::ACCENT
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

    ui.add_space(6.0);
    changed
}

fn field_label(ui: &mut Ui, field: &FieldSpec, invalid: bool) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(field.label)
                .color(if invalid { theme::DANGER } else { theme::MUTED })
                .size(10.5),
        );
        if field.required {
            ui.label(
                RichText::new("*")
                    .color(if invalid {
                        theme::DANGER
                    } else {
                        theme::ACCENT
                    })
                    .size(10.5),
            );
        }
    });
    ui.add_space(3.0);
}

fn validate(step: &StepSpec, draft: &IntakeDraft) -> Option<String> {
    let missing: Vec<&str> = step
        .fields
        .iter()
        .filter(|field| field.required && !field_filled(draft, field))
        .map(|field| field.label)
        .collect();

    if missing.is_empty() {
        None
    } else {
        Some(format!("Complete required fields: {}", missing.join(", ")))
    }
}

fn field_filled(draft: &IntakeDraft, field: &FieldSpec) -> bool {
    if field.kind == FieldKind::Checkbox {
        draft.is_checked(field.key)
    } else {
        !draft.get(field.key).trim().is_empty()
    }
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
            .circle_stroke(rect.center(), 27.0, Stroke::new(1.0, theme::ACCENT));
        let tick = Stroke::new(2.0, theme::ACCENT);
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
                .color(theme::MUTED)
                .size(13.0),
        );
        ui.add_space(4.0);
        theme::pill(ui, completed.kind.short_label(), kind_colour(completed.kind));
        ui.add_space(4.0);
        ui.label(
            RichText::new("The contact is saved with status Onboarding and will sync when the server API is available.")
                .color(theme::FAINT)
                .size(10.5),
        );

        ui.add_space(20.0);
        ui.horizontal(|ui| {
            if ui.add(theme::primary_button("View contact")).clicked() {
                let page = match completed.kind {
                    IntakeKind::Client => Page::Clients,
                    IntakeKind::Worker => Page::Workers,
                    IntakeKind::Other => Page::Contacts,
                };
                action = Some(IntakeAction::ViewContact {
                    page,
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
            .rect_filled(rect, CornerRadius::same(14), theme::SURFACE_RAISED);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "📝",
            FontId::proportional(22.0),
            theme::TEXT,
        );
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(title).font(theme::strong_font(26.0)));
            ui.label(RichText::new(subtitle).color(theme::MUTED).size(13.0));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            theme::pill(ui, "WIZARDS", theme::ACCENT);
        });
    });
}

fn section_header(ui: &mut Ui, title: &str, count: usize) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(theme::strong_font(15.0)));
        theme::pill(ui, &count.to_string(), theme::FAINT);
    });
    ui.add_space(8.0);
}

fn progress(ui: &mut Ui, steps: &[StepSpec], current: usize) {
    let fraction = (current + 1) as f32 / steps.len().max(1) as f32;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 5.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(3), theme::SURFACE_RAISED);
    let filled = egui::Rect::from_min_size(rect.min, vec2(rect.width() * fraction, 5.0));
    ui.painter()
        .rect_filled(filled, CornerRadius::same(3), theme::ACCENT);
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
        ui.label(RichText::new(message).color(theme::MUTED).size(12.0));
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
        IntakeKind::Client => theme::ACCENT_BLUE,
        IntakeKind::Worker => theme::ACCENT,
        IntakeKind::Other => theme::WARNING,
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
