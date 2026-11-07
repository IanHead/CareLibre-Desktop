//! Contact record editor, loaded from and saved to the local database.

use std::collections::BTreeMap;

use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea, Stroke, Ui,
    vec2,
};

use crate::contact::{ContactRecord, ContactUpdate, CustomFieldDef};
use crate::intake::IntakeKind;
use crate::pages::intake::fields::{self, FieldKind, FieldSpec};
use crate::store::ContactKind;
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactEditorAction {
    Save,
    Cancel,
}

pub struct ContactEditorState {
    pub open: bool,
    pub loading: bool,
    pub saving: bool,
    pub request_seq: u64,
    pub request_id: Option<u64>,
    pub contact_id: String,
    pub record: Option<ContactRecord>,
    pub name: String,
    pub kind: ContactKind,
    pub status: String,
    pub email: String,
    pub phone: String,
    pub division: String,
    pub profile: BTreeMap<String, String>,
    pub custom: BTreeMap<String, String>,
    pub error: Option<String>,
}

impl ContactEditorState {
    pub fn new() -> Self {
        Self {
            open: false,
            loading: false,
            saving: false,
            request_seq: 0,
            request_id: None,
            contact_id: String::new(),
            record: None,
            name: String::new(),
            kind: ContactKind::Client,
            status: "Active".to_owned(),
            email: String::new(),
            phone: String::new(),
            division: String::new(),
            profile: BTreeMap::new(),
            custom: BTreeMap::new(),
            error: None,
        }
    }

    pub fn begin(&mut self, id: String) -> u64 {
        self.request_seq += 1;
        self.request_id = Some(self.request_seq);
        self.contact_id = id;
        self.open = true;
        self.loading = true;
        self.saving = false;
        self.record = None;
        self.error = None;
        self.request_seq
    }

    pub fn apply(&mut self, record: ContactRecord) {
        self.name = record.summary.name.clone();
        self.kind = record.summary.kind;
        self.status = record.summary.status.clone();
        self.email = record.summary.email.clone().unwrap_or_default();
        self.phone = record.summary.phone.clone().unwrap_or_default();
        self.division = record.summary.division.clone().unwrap_or_default();
        self.profile = record.profile.clone();
        self.custom = record.custom_values.clone();
        self.contact_id = record.summary.id.clone();
        self.record = Some(record);
        self.loading = false;
        self.saving = false;
        self.error = None;
    }

    pub fn update(&self) -> Result<ContactUpdate, String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("A contact needs a display name.".to_owned());
        }

        Ok(ContactUpdate {
            id: self.contact_id.clone(),
            name: name.to_owned(),
            kind: self.kind,
            email: optional(&self.email),
            phone: optional(&self.phone),
            division: optional(&self.division),
            status: self.status.clone(),
            profile: self.profile.clone(),
            custom_values: self.custom.clone(),
        })
    }

    pub fn close(&mut self) {
        self.open = false;
        self.loading = false;
        self.saving = false;
    }
}

impl Default for ContactEditorState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn show(ctx: &egui::Context, state: &mut ContactEditorState) -> Option<ContactEditorAction> {
    if !state.open {
        return None;
    }

    let mut action = None;

    egui::Window::new("Edit contact")
        .id(egui::Id::unique("contact_editor"))
        .title_bar(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -6.0))
        .collapsible(false)
        .resizable(true)
        .default_width(760.0)
        .default_height(780.0)
        .max_height(820.0)
        .frame(
            Frame::new()
                .fill(theme::SURFACE)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::same(14)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Edit contact").font(theme::strong_font(18.0)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("×").color(theme::MUTED).size(14.0))
                                .frame(false)
                                .min_size(vec2(28.0, 28.0)),
                        )
                        .clicked()
                    {
                        state.close();
                        action = Some(ContactEditorAction::Cancel);
                    }
                });
            });
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(8.0);

            if state.loading {
                ui.label(
                    RichText::new("Loading contact from the local database…")
                        .color(theme::MUTED)
                        .size(12.0),
                );
                return;
            }

            if let Some(error) = &state.error {
                error_box(ui, error);
                ui.add_space(8.0);
            }

            ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(620.0)
                .show(ui, |ui| {
                    core_card(ui, state);
                    ui.add_space(12.0);
                    custom_fields_card(ui, state);
                    ui.add_space(12.0);

                    let intake_kind = intake_kind(state.kind);
                    for step in fields::steps(intake_kind) {
                        profile_card(ui, state, step);
                        ui.add_space(12.0);
                    }
                });

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    state.close();
                    action = Some(ContactEditorAction::Cancel);
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = if state.saving {
                        "Saving…"
                    } else {
                        "Save contact"
                    };
                    if ui
                        .add_enabled(!state.saving, theme::primary_button(label))
                        .clicked()
                    {
                        action = Some(ContactEditorAction::Save);
                    }
                });
            });
        });

    action
}

fn core_card(ui: &mut Ui, state: &mut ContactEditorState) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Core details").font(theme::strong_font(15.0)));
        ui.add_space(8.0);

        field_label(ui, "Display name", true);
        let response = ui.add_sized(
            [ui.available_width(), 34.0],
            egui::TextEdit::singleline(&mut state.name)
                .hint_text("Contact name")
                .margin(Margin::symmetric(10, 8)),
        );
        name_widget(&response, "Display name");
        ui.add_space(8.0);

        ui.columns(3, |cols| {
            field_label(&mut cols[0], "Contact type", false);
            kind_select(&mut cols[0], state);

            field_label(&mut cols[1], "Status", false);
            select(
                &mut cols[1],
                "contact-status",
                "Status",
                &mut state.status,
                &["Active", "Onboarding", "Inactive"],
            );

            text_field(&mut cols[2], "Division", &mut state.division, false);
        });

        ui.add_space(4.0);
        ui.columns(2, |cols| {
            text_field(&mut cols[0], "Email", &mut state.email, false);
            text_field(&mut cols[1], "Phone", &mut state.phone, false);
        });
        ui.label(
            RichText::new(format!("Contact id: {}", state.contact_id))
                .color(theme::FAINT)
                .size(9.0)
                .monospace(),
        );
    });
}

fn profile_card(ui: &mut Ui, state: &mut ContactEditorState, step: &fields::StepSpec) {
    let visible: Vec<&FieldSpec> = step
        .fields
        .iter()
        .filter(|field| {
            !matches!(
                field.key,
                "first_name" | "last_name" | "accurate" | "consent_share" | "consent_plan"
            )
        })
        .collect();

    if visible.is_empty() {
        return;
    }

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(step.title).font(theme::strong_font(14.0)));
        ui.label(
            RichText::new(step.description)
                .color(theme::MUTED)
                .size(10.5),
        );
        ui.add_space(8.0);

        let mut pending: Option<&FieldSpec> = None;
        for field in visible {
            if field.wide {
                if let Some(first) = pending.take() {
                    render_profile_field(ui, &mut state.profile, &state.contact_id, first);
                }
                render_profile_field(ui, &mut state.profile, &state.contact_id, field);
                continue;
            }

            if let Some(first) = pending.take() {
                ui.columns(2, |cols| {
                    render_profile_field(
                        &mut cols[0],
                        &mut state.profile,
                        &state.contact_id,
                        first,
                    );
                    render_profile_field(
                        &mut cols[1],
                        &mut state.profile,
                        &state.contact_id,
                        field,
                    );
                });
            } else {
                pending = Some(field);
            }
        }
        if let Some(field) = pending {
            render_profile_field(ui, &mut state.profile, &state.contact_id, field);
        }
    });
}

fn custom_fields_card(ui: &mut Ui, state: &mut ContactEditorState) {
    let Some(record) = &state.record else {
        return;
    };
    if record.custom_fields.is_empty() {
        return;
    }

    let mut sections: Vec<(String, Vec<&CustomFieldDef>)> = Vec::new();
    for field in &record.custom_fields {
        let section = field
            .section
            .clone()
            .unwrap_or_else(|| "Custom fields".to_owned());
        if let Some((_, fields)) = sections.iter_mut().find(|(name, _)| name == &section) {
            fields.push(field);
        } else {
            sections.push((section, vec![field]));
        }
    }

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Custom fields").font(theme::strong_font(15.0)));
        ui.label(
            RichText::new(
                "Custom fields are organisation metadata loaded from the database and rendered dynamically.",
            )
            .color(theme::MUTED)
            .size(10.5),
        );
        ui.add_space(8.0);

        for (section, fields) in sections {
            ui.label(
                RichText::new(section.to_uppercase())
                    .color(theme::FAINT)
                    .font(theme::strong_font(9.5)),
            );
            ui.add_space(4.0);
            for field in fields {
                render_custom_field(ui, &mut state.custom, field);
                ui.add_space(7.0);
            }
            ui.add_space(4.0);
        }
    });
}

fn render_profile_field(
    ui: &mut Ui,
    values: &mut BTreeMap<String, String>,
    contact_id: &str,
    field: &FieldSpec,
) {
    let mut value = values.get(field.key).cloned().unwrap_or_default();

    if field.kind == FieldKind::Checkbox {
        let mut checked = value == "true";
        ui.horizontal(|ui| {
            if ui
                .checkbox(&mut checked, RichText::new(field.label).size(11.0))
                .changed()
            {
                values.insert(field.key.to_owned(), checked.to_string());
            }
            if field.required {
                ui.label(RichText::new("*").color(theme::ACCENT).size(10.5));
            }
        });
        return;
    }

    field_label(ui, field.label, field.required);

    match field.kind {
        FieldKind::Multiline => {
            let response = ui.add(
                egui::TextEdit::multiline(&mut value)
                    .hint_text(field.placeholder)
                    .desired_rows(3)
                    .desired_width(ui.available_width())
                    .margin(Margin::symmetric(10, 8)),
            );
            name_widget(&response, field.label);
            if response.changed() {
                values.insert(field.key.to_owned(), value);
            }
        }
        FieldKind::Select => {
            let id = format!("editor-{contact_id}-{}", field.key);
            let mut current = value;
            let previous = current.clone();
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
            name_widget(&response.response, field.label);
            if current != previous {
                values.insert(field.key.to_owned(), current);
            }
        }
        _ => {
            let response = ui.add(
                egui::TextEdit::singleline(&mut value)
                    .hint_text(field.placeholder)
                    .desired_width(ui.available_width())
                    .margin(Margin::symmetric(10, 8)),
            );
            name_widget(&response, field.label);
            if response.changed() {
                values.insert(field.key.to_owned(), value);
            }
        }
    }
    ui.add_space(6.0);
}

fn render_custom_field(ui: &mut Ui, values: &mut BTreeMap<String, String>, field: &CustomFieldDef) {
    let mut value = values.get(&field.id).cloned().unwrap_or_default();
    field_label(ui, &field.label, field.required);

    match field.field_type.as_str() {
        "multiline" => {
            let response = ui.add(
                egui::TextEdit::multiline(&mut value)
                    .desired_rows(2)
                    .desired_width(ui.available_width())
                    .margin(Margin::symmetric(10, 8)),
            );
            name_widget(&response, &field.label);
            if response.changed() {
                values.insert(field.id.clone(), value);
            }
        }
        "select" => {
            let previous = value.clone();
            let response = egui::ComboBox::from_id_salt(format!("custom-{}", field.id))
                .selected_text(if value.is_empty() {
                    "Select…"
                } else {
                    &value
                })
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for option in &field.options {
                        ui.selectable_value(&mut value, option.clone(), option.as_str());
                    }
                });
            name_widget(&response.response, &field.label);
            if value != previous {
                values.insert(field.id.clone(), value);
            }
        }
        "checkbox" => {
            let mut checked = value == "true";
            if ui
                .checkbox(&mut checked, RichText::new(&field.label).size(11.0))
                .changed()
            {
                values.insert(field.id.clone(), checked.to_string());
            }
        }
        _ => {
            let response = ui.add(
                egui::TextEdit::singleline(&mut value)
                    .desired_width(ui.available_width())
                    .margin(Margin::symmetric(10, 8)),
            );
            name_widget(&response, &field.label);
            if response.changed() {
                values.insert(field.id.clone(), value);
            }
        }
    }
}

fn kind_select(ui: &mut Ui, state: &mut ContactEditorState) {
    let mut label = state.kind.label().to_owned();
    let previous = label.clone();
    let response = egui::ComboBox::from_id_salt("contact-kind")
        .selected_text(&label)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for kind in [
                ContactKind::Client,
                ContactKind::Worker,
                ContactKind::Dual,
                ContactKind::Other,
            ] {
                ui.selectable_value(&mut label, kind.label().to_owned(), kind.label());
            }
        });
    name_widget(&response.response, "Contact type");
    if label != previous {
        state.kind = match label.as_str() {
            "Worker" => ContactKind::Worker,
            "Dual role" => ContactKind::Dual,
            "Other" => ContactKind::Other,
            _ => ContactKind::Client,
        };
    }
}

fn select(ui: &mut Ui, id: &str, label: &str, value: &mut String, options: &[&str]) {
    let selected = if value.is_empty() {
        "Select…".to_owned()
    } else {
        value.clone()
    };
    let response = egui::ComboBox::from_id_salt(id.to_owned())
        .selected_text(selected)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for option in options {
                ui.selectable_value(&mut *value, (*option).to_owned(), *option);
            }
        });
    name_widget(&response.response, label);
}

fn text_field(ui: &mut Ui, label: &str, value: &mut String, required: bool) {
    field_label(ui, label, required);
    let response = ui.add_sized(
        [ui.available_width(), 34.0],
        egui::TextEdit::singleline(value).margin(Margin::symmetric(10, 8)),
    );
    name_widget(&response, label);
    ui.add_space(6.0);
}

fn name_widget(response: &egui::Response, label: &str) {
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
}

fn field_label(ui: &mut Ui, label: &str, required: bool) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(theme::MUTED).size(10.5));
        if required {
            ui.label(RichText::new("*").color(theme::ACCENT).size(10.5));
        }
    });
    ui.add_space(3.0);
}

fn error_box(ui: &mut Ui, message: &str) {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(
            theme::DANGER.r(),
            theme::DANGER.g(),
            theme::DANGER.b(),
            20,
        ))
        .stroke(Stroke::new(1.0, theme::DANGER.gamma_multiply(0.55)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(message).color(theme::DANGER).size(11.0));
        });
}

fn intake_kind(kind: ContactKind) -> IntakeKind {
    match kind {
        ContactKind::Worker => IntakeKind::Worker,
        ContactKind::Other => IntakeKind::Other,
        ContactKind::Client | ContactKind::Dual => IntakeKind::Client,
    }
}

fn optional(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}
