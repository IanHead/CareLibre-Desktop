//! Native, free-floating contact editor windows backed by the server.

use std::collections::BTreeMap;

use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea, Stroke, Ui,
    vec2,
};
use serde_json::{Map, Value};

use crate::contact::{ApiContactDetail, ApiContactWrite, ApiCustomField};
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactEditorAction {
    Save,
    Cancel,
    PopOut,
    Dock,
}

pub struct ContactWindowState {
    pub contact_id: String,
    pub request_seq: u64,
    pub request_id: Option<u64>,
    pub loading: bool,
    pub saving: bool,
    pub detail: Option<ApiContactDetail>,
    pub display_name: String,
    pub given_name: String,
    pub family_name: String,
    pub preferred_name: String,
    pub pronouns: String,
    pub date_of_birth: String,
    pub email: String,
    pub phone: String,
    pub notes: String,
    pub role: String,
    pub status: String,
    pub details: Map<String, Value>,
    pub custom: BTreeMap<String, String>,
    pub error: Option<String>,
    pub close_requested: bool,
    pub popped_out: bool,
}

impl ContactWindowState {
    pub fn new(contact_id: String) -> Self {
        let mut state = Self {
            contact_id,
            request_seq: 0,
            request_id: None,
            loading: true,
            saving: false,
            detail: None,
            display_name: String::new(),
            given_name: String::new(),
            family_name: String::new(),
            preferred_name: String::new(),
            pronouns: String::new(),
            date_of_birth: String::new(),
            email: String::new(),
            phone: String::new(),
            notes: String::new(),
            role: "contact".to_owned(),
            status: "Active".to_owned(),
            details: Map::new(),
            custom: BTreeMap::new(),
            error: None,
            close_requested: false,
            popped_out: false,
        };
        state.request_seq = 1;
        state.request_id = Some(1);
        state
    }

    pub fn apply(&mut self, detail: ApiContactDetail) {
        let contact = &detail.contact;
        self.display_name = contact.display_name.clone();
        self.given_name = contact.given_name.clone().unwrap_or_default();
        self.family_name = contact.family_name.clone().unwrap_or_default();
        self.preferred_name = contact.preferred_name.clone().unwrap_or_default();
        self.pronouns = contact.pronouns.clone().unwrap_or_default();
        self.date_of_birth = contact.date_of_birth.clone().unwrap_or_default();
        self.email = contact.email.clone().unwrap_or_default();
        self.phone = contact.phone.clone().unwrap_or_default();
        self.notes = contact.notes.clone().unwrap_or_default();
        self.role = contact.role.clone();
        self.status = if contact.is_active {
            contact
                .details
                .get("contact_status")
                .and_then(Value::as_str)
                .unwrap_or("Active")
                .to_owned()
        } else {
            "Inactive".to_owned()
        };
        self.details = contact.details.as_object().cloned().unwrap_or_default();
        self.custom = detail
            .custom_fields
            .iter()
            .map(|field| {
                let value = detail.custom_values.get(&field.id);
                (field.id.clone(), display_value(field, value))
            })
            .collect();
        self.detail = Some(detail);
        self.loading = false;
        self.saving = false;
        self.error = None;
        self.close_requested = false;
    }

    pub fn title(&self) -> String {
        if self.display_name.is_empty() {
            "Contact — CareLibre".to_owned()
        } else {
            format!("{} — CareLibre", self.display_name)
        }
    }

    pub fn build_write(&self) -> Result<ApiContactWrite, String> {
        let display_name = self.display_name.trim();
        if display_name.is_empty() {
            return Err("A contact needs a display name.".to_owned());
        }

        let mut details = self.details.clone();
        if self.role == "contact" {
            details.remove("contact_role");
        } else {
            details.insert("contact_role".to_owned(), Value::String(self.role.clone()));
        }
        if self.status == "Active" {
            details.remove("contact_status");
        } else {
            details.insert(
                "contact_status".to_owned(),
                Value::String(self.status.clone()),
            );
        }

        let mut custom_fields = BTreeMap::new();
        if let Some(detail) = &self.detail {
            for field in &detail.custom_fields {
                let text = self.custom.get(&field.id).cloned().unwrap_or_default();
                custom_fields.insert(field.id.clone(), stored_value(field, &text));
            }
        }

        Ok(ApiContactWrite {
            display_name: display_name.to_owned(),
            given_name: optional(&self.given_name),
            family_name: optional(&self.family_name),
            preferred_name: optional(&self.preferred_name),
            pronouns: optional(&self.pronouns),
            date_of_birth: optional(&self.date_of_birth),
            email: optional(&self.email),
            phone: optional(&self.phone),
            notes: optional(&self.notes),
            details: Value::Object(details),
            is_active: self.status != "Inactive",
            custom_fields,
        })
    }

    pub fn request_id(&mut self) -> u64 {
        self.request_seq += 1;
        self.request_id = Some(self.request_seq);
        self.request_seq
    }
}

pub fn show_viewport(
    ui: &mut Ui,
    state: &mut ContactWindowState,
    client_side_decorations: bool,
) -> Option<ContactEditorAction> {
    if ui.ctx().input(|i| i.viewport().close_requested()) {
        state.close_requested = true;
        return Some(ContactEditorAction::Cancel);
    }

    let mut action = None;

    if client_side_decorations {
        egui::Panel::top("contact-window-chrome")
            .exact_size(crate::chrome::HEIGHT)
            .show_separator_line(false)
            .frame(Frame::new().fill(theme::SIDEBAR).inner_margin(Margin::ZERO))
            .show(ui, |ui| {
                let ctx = ui.ctx().clone();
                let title = state.title();
                crate::chrome::title_bar(ui, &ctx, &title);
            });
    }

    egui::Panel::bottom("contact-editor-footer")
        .exact_size(60.0)
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(theme::BG)
                .inner_margin(Margin::symmetric(16, 10))
                .stroke(Stroke::new(1.0, theme::BORDER)),
        )
        .show(ui, |ui| {
            if let Some(next) = footer(ui, state) {
                action = Some(next);
            }
        });

    egui::CentralPanel::default()
        .frame(Frame::new().fill(theme::BG).inner_margin(Margin::same(16)))
        .show(ui, |ui| {
            if let Some(next) = header(ui, state) {
                action = Some(next);
            }
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(8.0);
            if let Some(next) = body(ui, state) {
                action = Some(next);
            }
        });

    if client_side_decorations {
        crate::chrome::resize_handles(ui.ctx());
    }

    action
}

pub fn show_modal(
    ctx: &egui::Context,
    state: &mut ContactWindowState,
) -> Option<ContactEditorAction> {
    let mut action = None;
    egui::Modal::new(egui::Id::unique(("contact-modal", &state.contact_id)))
        .backdrop_color(Color32::from_black_alpha(190))
        .frame(
            Frame::new()
                .fill(theme::SURFACE)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(Margin::same(18)),
        )
        .show(ctx, |ui| {
            ui.set_width(820.0);
            ui.set_min_height(620.0);

            if let Some(next) = header(ui, state) {
                action = Some(next);
            }
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(8.0);

            ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(530.0)
                .show(ui, |ui| {
                    if let Some(next) = body(ui, state) {
                        action = Some(next);
                    }
                });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);
            if let Some(next) = footer(ui, state) {
                action = Some(next);
            }
        });

    action
}

fn header(ui: &mut Ui, state: &mut ContactWindowState) -> Option<ContactEditorAction> {
    let mut action = None;

    ui.horizontal(|ui| {
        let title = if state.display_name.is_empty() {
            "Contact record".to_owned()
        } else {
            state.display_name.clone()
        };
        ui.label(RichText::new(title).font(theme::strong_font(18.0)));
        role_pill(ui, &state.role);
        status_pill(ui, &state.status);

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add(
                    egui::Button::new(RichText::new("×").color(theme::MUTED).size(16.0))
                        .frame(false)
                        .min_size(vec2(28.0, 28.0)),
                )
                .clicked()
            {
                state.close_requested = true;
                action = Some(ContactEditorAction::Cancel);
            }

            let (label, next) = if state.popped_out {
                ("Dock", ContactEditorAction::Dock)
            } else {
                ("Pop out", ContactEditorAction::PopOut)
            };
            if ui
                .add(
                    egui::Button::new(RichText::new(label).color(theme::ACCENT).size(11.0))
                        .frame(false),
                )
                .clicked()
            {
                action = Some(next);
            }

            if state.saving {
                ui.label(RichText::new("Saving…").color(theme::WARNING).size(10.5));
            }
        });
    });

    action
}

fn body(ui: &mut Ui, state: &mut ContactWindowState) -> Option<ContactEditorAction> {
    if state.loading {
        ui.label(
            RichText::new("Loading contact from the server…")
                .color(theme::MUTED)
                .size(12.0),
        );
        return None;
    }

    if let Some(error) = &state.error {
        error_box(ui, error);
        ui.add_space(8.0);
    }

    core_card(ui, state);
    ui.add_space(12.0);
    profile_card(ui, state);
    None
}

fn footer(ui: &mut Ui, state: &mut ContactWindowState) -> Option<ContactEditorAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        if ui.add(theme::secondary_button("Cancel")).clicked() {
            state.close_requested = true;
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
    action
}

fn core_card(ui: &mut Ui, state: &mut ContactWindowState) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Stock fields").font(theme::strong_font(15.0)));
        ui.label(
            RichText::new("Core contact data stored by the server.")
                .color(theme::MUTED)
                .size(10.5),
        );
        ui.add_space(8.0);

        field_label(ui, "Display name", true);
        named(
            &ui.add(
                egui::TextEdit::singleline(&mut state.display_name)
                    .hint_text("Contact name")
                    .desired_width(ui.available_width())
                    .margin(Margin::symmetric(10, 8)),
            ),
            "Display name",
        );
        ui.add_space(8.0);

        ui.columns(2, |cols| {
            text_field(&mut cols[0], "Given name", &mut state.given_name);
            text_field(&mut cols[1], "Family name", &mut state.family_name);
        });
        ui.columns(2, |cols| {
            text_field(&mut cols[0], "Preferred name", &mut state.preferred_name);
            text_field(&mut cols[1], "Pronouns", &mut state.pronouns);
        });
        ui.columns(2, |cols| {
            text_field(
                &mut cols[0],
                "Date of birth (YYYY-MM-DD)",
                &mut state.date_of_birth,
            );
            text_field(&mut cols[1], "Email", &mut state.email);
        });
        ui.columns(2, |cols| {
            text_field(&mut cols[0], "Phone", &mut state.phone);
            field_label(&mut cols[1], "Role", false);
            role_select(&mut cols[1], state);
        });
        ui.add_space(4.0);
        ui.columns(2, |cols| {
            field_label(&mut cols[0], "Status", false);
            status_select(&mut cols[0], state);
        });

        ui.add_space(8.0);
        field_label(ui, "Notes", false);
        named(
            &ui.add(
                egui::TextEdit::multiline(&mut state.notes)
                    .desired_rows(2)
                    .desired_width(ui.available_width())
                    .margin(Margin::symmetric(10, 8)),
            ),
            "Notes",
        );
    });
}

fn profile_card(ui: &mut Ui, state: &mut ContactWindowState) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Profile details").font(theme::strong_font(15.0)));
        ui.label(
            RichText::new(
                "Profile data and organisation-defined fields are rendered together from the server.",
            )
            .color(theme::MUTED)
            .size(10.5),
        );
        ui.add_space(8.0);

        let keys: Vec<String> = state
            .details
            .keys()
            .filter(|key| key.as_str() != "contact_role" && key.as_str() != "contact_status")
            .cloned()
            .collect();

        if keys.is_empty() && state.custom.is_empty() {
            ui.label(
                RichText::new("No additional profile data yet.")
                    .color(theme::FAINT)
                    .size(11.0),
            );
            return;
        }

        for key in keys {
            if let Some(value) = state.details.get_mut(&key) {
                render_detail_value(ui, &key, value, 0);
                ui.add_space(6.0);
            }
        }

        if let Some(detail) = &state.detail {
            for field in &detail.custom_fields {
                let mut text = state.custom.get(&field.id).cloned().unwrap_or_default();
                custom_field(ui, field, &mut text);
                state.custom.insert(field.id.clone(), text);
                ui.add_space(6.0);
            }
        }
    });
}

fn render_detail_value(ui: &mut Ui, key: &str, value: &mut Value, depth: usize) {
    match value {
        Value::Object(map) => {
            ui.label(
                RichText::new(label_for(key))
                    .color(theme::MUTED)
                    .font(theme::strong_font(11.0)),
            );
            ui.indent(("detail", key, depth), |ui| {
                let mut keys: Vec<String> = map.keys().cloned().collect();
                keys.sort();
                for child in keys {
                    if let Some(value) = map.get_mut(&child) {
                        render_detail_value(ui, &child, value, depth + 1);
                        ui.add_space(6.0);
                    }
                }
            });
        }
        Value::Bool(checked) => {
            let mut checked = *checked;
            if ui
                .checkbox(&mut checked, RichText::new(label_for(key)).size(11.0))
                .changed()
            {
                *value = Value::Bool(checked);
            }
        }
        Value::Number(number) => {
            let mut text = number.to_string();
            text_field(ui, &label_for(key), &mut text);
            if let Ok(parsed) = text.parse::<f64>()
                && let Some(number) = serde_json::Number::from_f64(parsed)
            {
                *value = Value::Number(number);
            }
        }
        Value::String(existing) => {
            let original = existing.clone();
            let mut text = original.clone();
            let long = should_wrap(key);
            if long {
                field_label(ui, label_for(key).as_str(), false);
                named(
                    &ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .desired_rows(2)
                            .desired_width(ui.available_width())
                            .margin(Margin::symmetric(10, 8)),
                    ),
                    label_for(key).as_str(),
                );
            } else {
                text_field(ui, label_for(key).as_str(), &mut text);
            }
            if text != original {
                *value = Value::String(text);
            }
        }
        Value::Array(items) => {
            let joined = items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| item.to_string())
                })
                .collect::<Vec<_>>()
                .join(", ");
            let mut text = joined;
            text_field(ui, &label_for(key), &mut text);
            let parts: Vec<Value> = text
                .split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(|part| Value::String(part.to_owned()))
                .collect();
            *value = Value::Array(parts);
        }
        Value::Null => {
            let mut text = String::new();
            text_field(ui, &label_for(key), &mut text);
            if !text.is_empty() {
                *value = Value::String(text);
            }
        }
    }
}

fn custom_field(ui: &mut Ui, field: &ApiCustomField, text: &mut String) {
    if let Some(help) = &field.help_text {
        ui.label(RichText::new(help).color(theme::FAINT).size(9.5));
    }

    match field.data_type.as_str() {
        "long_text" => {
            field_label(ui, &field.label, field.is_required);
            named(
                &ui.add(
                    egui::TextEdit::multiline(text)
                        .desired_rows(2)
                        .desired_width(ui.available_width())
                        .margin(Margin::symmetric(10, 8)),
                ),
                &field.label,
            );
        }
        "boolean" => {
            let mut checked = text == "true";
            if ui
                .checkbox(&mut checked, RichText::new(&field.label).size(11.0))
                .changed()
            {
                *text = checked.to_string();
            }
        }
        "select" => {
            field_label(ui, &field.label, field.is_required);
            let options = field.options.clone().unwrap_or_default();
            combo(
                ui,
                &format!("custom-select-{}", field.id),
                text,
                &options,
                &field.label,
            );
        }
        "multiselect" => {
            field_label(ui, &field.label, field.is_required);
            let options = field.options.clone().unwrap_or_default();
            multi_select(
                ui,
                &format!("custom-multi-{}", field.id),
                text,
                &options,
                &field.label,
            );
        }
        _ => {
            text_field(ui, &field.label, text);
        }
    }
    ui.add_space(4.0);
}

fn multi_select(ui: &mut Ui, id: &str, value: &mut String, options: &[String], label: &str) {
    let selected: Vec<String> = value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect();
    let selected_label = if selected.is_empty() {
        "Select…".to_owned()
    } else {
        selected.join(", ")
    };

    let response = egui::ComboBox::from_id_salt(id.to_owned())
        .selected_text(selected_label)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for option in options {
                let mut checked = selected.iter().any(|item| item == option);
                if ui.checkbox(&mut checked, option).changed() {
                    let mut next = selected.clone();
                    if checked {
                        next.push(option.clone());
                    } else {
                        next.retain(|item| item != option);
                    }
                    *value = next.join(", ");
                }
            }
        });
    named(&response.response, label);
}

fn combo(ui: &mut Ui, id: &str, value: &mut String, options: &[String], label: &str) {
    let mut current = value.clone();
    let response = egui::ComboBox::from_id_salt(id.to_owned())
        .selected_text(if current.is_empty() {
            "Select…".to_owned()
        } else {
            current.clone()
        })
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for option in options {
                ui.selectable_value(&mut current, option.clone(), option);
            }
        });
    named(&response.response, label);
    if current != *value {
        *value = current;
    }
}

fn role_select(ui: &mut Ui, state: &mut ContactWindowState) {
    let labels = ["client", "worker", "dual", "other", "contact"];
    let mut current = state.role.clone();
    let response = egui::ComboBox::from_id_salt("contact-role")
        .selected_text(role_label(&current))
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for role in labels {
                ui.selectable_value(&mut current, role.to_owned(), role_label(role));
            }
        });
    named(&response.response, "Role");
    state.role = current;
}

fn status_select(ui: &mut Ui, state: &mut ContactWindowState) {
    let mut current = state.status.clone();
    let response = egui::ComboBox::from_id_salt("contact-status")
        .selected_text(&current)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for option in ["Active", "Onboarding", "Inactive"] {
                ui.selectable_value(&mut current, option.to_owned(), option);
            }
        });
    named(&response.response, "Status");
    state.status = current;
}

fn role_pill(ui: &mut Ui, role: &str) {
    let colour = match role {
        "client" => theme::ACCENT_BLUE,
        "worker" => theme::ACCENT,
        "dual" => theme::WARNING,
        _ => theme::FAINT,
    };
    theme::pill(ui, &role_label(role).to_uppercase(), colour);
}

fn status_pill(ui: &mut Ui, status: &str) {
    let colour = match status {
        "Active" => theme::SUCCESS,
        "Onboarding" => theme::WARNING,
        _ => theme::FAINT,
    };
    theme::pill(ui, &status.to_uppercase(), colour);
}

fn role_label(role: &str) -> &'static str {
    match role {
        "client" => "Client",
        "worker" => "Worker",
        "dual" => "Dual role",
        "other" => "Other contact",
        _ => "Contact",
    }
}

fn label_for(key: &str) -> String {
    let mapped = match key {
        "line1" => "Address line 1",
        "line2" => "Address line 2",
        "suburb" => "Suburb",
        "postcode" => "Postcode",
        "start_date" => "Start date",
        "end_date" => "End date",
        "contact_role" => "Role",
        "contact_status" => "Status",
        "given_name" => "Given name",
        "family_name" => "Family name",
        "preferred_name" => "Preferred name",
        "date_of_birth" => "Date of birth",
        "primary_disability" => "Primary disability",
        "health_conditions" => "Health conditions",
        "support_coordinator" => "Support coordinator",
        "plan_manager" => "Plan manager",
        "ndis_number" => "NDIS number",
        "employee_number" => "Employee number",
        "plan_end_date" => "Plan end date",
        "risk_level" => "Risk level",
        _ => key,
    };
    humanize(mapped)
}

fn humanize(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for (index, character) in value.replace('_', " ").chars().enumerate() {
        if index == 0 {
            out.extend(character.to_uppercase());
        } else {
            out.push(character);
        }
    }
    out
}

fn should_wrap(key: &str) -> bool {
    matches!(
        key,
        "notes"
            | "health_conditions"
            | "medications"
            | "allergies"
            | "behaviours"
            | "triggers"
            | "strategies"
            | "risk_notes"
            | "communication"
            | "mobility"
            | "support_needs"
            | "intake_notes"
            | "worker_notes"
    )
}

fn text_field(ui: &mut Ui, label: &str, value: &mut String) {
    if !label.is_empty() {
        field_label(ui, label, false);
    }
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .desired_width(ui.available_width())
            .margin(Margin::symmetric(10, 8)),
    );
    if !label.is_empty() {
        named(&response, label);
    }
    ui.add_space(4.0);
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

fn named(response: &egui::Response, label: &str) {
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
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

fn optional(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn display_value(_field: &ApiCustomField, value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| item.to_string())
            })
            .collect::<Vec<_>>()
            .join(", "),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn stored_value(field: &ApiCustomField, text: &str) -> Value {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Value::Null;
    }
    match field.data_type.as_str() {
        "integer" => trimmed
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(trimmed.to_owned())),
        "decimal" => trimmed
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(trimmed.to_owned())),
        "boolean" => Value::Bool(trimmed.eq_ignore_ascii_case("true")),
        "multiselect" => Value::Array(
            trimmed
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(|item| Value::String(item.to_owned()))
                .collect(),
        ),
        _ => Value::String(trimmed.to_owned()),
    }
}
