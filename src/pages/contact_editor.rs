//! Native, free-floating contact editor windows backed by the server.

use std::collections::BTreeMap;

use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea, Stroke, Ui,
    vec2,
};
use serde_json::{Map, Value};

use crate::contact::{
    ApiContactDetail, ApiContactWrite, ApiCustomField, ApiGraphNode, ApiRelationship, PartyKind,
    relationship_role_label,
};
use crate::theme;

/// The contact and organisation roles that make a contact a client: the
/// organisation provides care to them.
pub const CLIENT_ROLES: (&str, &str) = ("care_recipient", "provider");

/// The contact and organisation roles that make a contact a worker.
pub const WORKER_ROLES: (&str, &str) = ("employee", "employer");

/// A relationship kind as shown in the UI, with the role each endpoint holds.
pub struct RelationshipKind {
    pub label: &'static str,
    pub subject_role: &'static str,
    pub linked_role: &'static str,
}

pub const RELATIONSHIP_KINDS: [RelationshipKind; 11] = [
    RelationshipKind {
        label: "Emergency Contact",
        subject_role: "person_supported",
        linked_role: "emergency_contact",
    },
    RelationshipKind {
        label: "Next of Kin",
        subject_role: "person_supported",
        linked_role: "next_of_kin",
    },
    RelationshipKind {
        label: "Primary Contact",
        subject_role: "person_supported",
        linked_role: "primary_contact",
    },
    RelationshipKind {
        label: "Representative",
        subject_role: "person_supported",
        linked_role: "representative",
    },
    RelationshipKind {
        label: "Support Coordinator",
        subject_role: "participant",
        linked_role: "support_coordinator",
    },
    RelationshipKind {
        label: "General Practitioner",
        subject_role: "patient",
        linked_role: "general_practitioner",
    },
    RelationshipKind {
        label: "Family",
        subject_role: "person_supported",
        linked_role: "family",
    },
    RelationshipKind {
        label: "Friend",
        subject_role: "person_supported",
        linked_role: "friend",
    },
    RelationshipKind {
        label: "Care Provider",
        subject_role: CLIENT_ROLES.0,
        linked_role: CLIENT_ROLES.1,
    },
    RelationshipKind {
        label: "Employer",
        subject_role: WORKER_ROLES.0,
        linked_role: WORKER_ROLES.1,
    },
    RelationshipKind {
        label: "Other",
        subject_role: "person_supported",
        linked_role: "other",
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewRelationshipContact {
    pub display_name: String,
    pub given_name: String,
    pub family_name: String,
    pub email: String,
    pub phone: String,
}

impl NewRelationshipContact {
    fn new() -> Self {
        Self {
            display_name: String::new(),
            given_name: String::new(),
            family_name: String::new(),
            email: String::new(),
            phone: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContactEditorAction {
    Save,
    Cancel,
    PopOut,
    Dock,
    OpenContact(String),
    CreateRelationship {
        from_role: String,
        to_kind: PartyKind,
        to_id: String,
        to_role: String,
        notes: String,
    },
    DeleteRelationship(String),
    CreateContact {
        from_role: String,
        to_role: String,
        notes: String,
        draft: NewRelationshipContact,
    },
}

pub struct ContactWindowState {
    pub contact_id: String,
    pub request_seq: u64,
    pub request_id: Option<u64>,
    pub relationship_request_id: Option<u64>,
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
    pub relationships: Vec<ApiRelationship>,
    pub relationships_loading: bool,
    pub relationship_kind: usize,
    pub relationship_target: Option<ApiGraphNode>,
    pub relationship_notes: String,
    pub relationship_pending: bool,
    pub relationship_removing: Option<String>,
    pub new_relationship_contact: Option<NewRelationshipContact>,
    pub relationship_error: Option<String>,
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
            relationship_request_id: None,
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
            relationships: Vec::new(),
            relationships_loading: false,
            relationship_kind: 0,
            relationship_target: None,
            relationship_notes: String::new(),
            relationship_pending: false,
            relationship_removing: None,
            new_relationship_contact: None,
            relationship_error: None,
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
        self.relationship_pending = false;
        self.relationship_removing = None;
        self.new_relationship_contact = None;
        self.relationship_error = None;
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

    pub fn apply_relationships(&mut self, items: Vec<ApiRelationship>) {
        self.relationships = items;
        self.relationships_loading = false;
        self.relationship_error = None;
    }

    pub fn relationships_failed(&mut self, message: String) {
        self.relationships_loading = false;
        self.relationship_error = Some(message);
    }

    /// Merge a relationship created or discovered by the server.
    pub fn relationship_linked(&mut self, edge: ApiRelationship) {
        if !self
            .relationships
            .iter()
            .any(|existing| existing.id == edge.id)
        {
            self.relationships.push(edge);
        }
        self.relationship_pending = false;
        self.relationship_removing = None;
        self.relationship_target = None;
        self.relationship_notes.clear();
        self.new_relationship_contact = None;
        self.relationship_error = None;
    }

    pub fn relationship_failed(&mut self, message: String) {
        self.relationship_pending = false;
        self.relationship_removing = None;
        self.relationship_error = Some(message);
    }

    pub fn relationship_removed(&mut self, id: &str) {
        self.relationships.retain(|edge| edge.id != id);
        self.relationship_removing = None;
    }

    pub fn build_write(&self) -> Result<ApiContactWrite, String> {
        let display_name = self.display_name.trim();
        if display_name.is_empty() {
            return Err("A contact needs a display name.".to_owned());
        }

        let mut details = self.details.clone();
        // Client and worker come from relationships, not a stored field.
        details.remove("contact_role");
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
    catalog: &[ApiGraphNode],
) -> Option<ContactEditorAction> {
    if ui.ctx().input(|i| i.viewport().close_requested()) {
        state.close_requested = true;
        return Some(ContactEditorAction::Cancel);
    }

    let mut action = None;

    if client_side_decorations {
        egui::Panel::top(format!("contact-window-chrome-{}", state.contact_id))
            .exact_size(crate::chrome::HEIGHT)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(theme::sidebar())
                    .inner_margin(Margin::ZERO),
            )
            .show(ui, |ui| {
                let ctx = ui.ctx().clone();
                let title = state.title();
                crate::chrome::title_bar(ui, &ctx, &title);
            });
    }

    egui::Panel::bottom(format!("contact-editor-footer-{}", state.contact_id))
        .exact_size(60.0)
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(theme::bg())
                .inner_margin(Margin::symmetric(16, 10))
                .stroke(Stroke::new(1.0, theme::border())),
        )
        .show(ui, |ui| {
            if let Some(next) = footer(ui, state) {
                action = Some(next);
            }
        });

    egui::CentralPanel::default()
        .frame(
            Frame::new()
                .fill(theme::bg())
                .inner_margin(Margin::same(16)),
        )
        .show(ui, |ui| {
            if let Some(next) = header(ui, state) {
                action = Some(next);
            }
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(8.0);
            if let Some(next) = body(ui, state, catalog) {
                action = Some(next);
            }
        });

    if client_side_decorations {
        crate::chrome::resize_handles(ui.ctx());
    }

    action
}

/// The docked editor body used by the right-hand record panel.
pub fn show_docked(
    ui: &mut Ui,
    state: &mut ContactWindowState,
    catalog: &[ApiGraphNode],
) -> Option<ContactEditorAction> {
    let mut action = None;

    egui::Panel::bottom("contact-dock-footer")
        .exact_size(58.0)
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(theme::bg())
                .inner_margin(Margin::symmetric(16, 10))
                .stroke(Stroke::new(1.0, theme::border())),
        )
        .show(ui, |ui| {
            if let Some(next) = footer(ui, state) {
                action = Some(next);
            }
        });

    egui::CentralPanel::default()
        .frame(
            Frame::new()
                .fill(theme::bg())
                .inner_margin(Margin::same(16)),
        )
        .show(ui, |ui| {
            if let Some(next) = header(ui, state) {
                action = Some(next);
            }
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(8.0);

            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if let Some(next) = body(ui, state, catalog) {
                        action = Some(next);
                    }
                });
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
                    egui::Button::new(RichText::new("×").color(theme::muted()).size(16.0))
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
                    egui::Button::new(RichText::new(label).color(theme::accent()).size(11.0))
                        .frame(false),
                )
                .clicked()
            {
                action = Some(next);
            }

            if state.saving {
                ui.label(RichText::new("Saving…").color(theme::warning()).size(10.5));
            }
        });
    });

    action
}

fn body(
    ui: &mut Ui,
    state: &mut ContactWindowState,
    catalog: &[ApiGraphNode],
) -> Option<ContactEditorAction> {
    if state.loading {
        ui.label(
            RichText::new("Loading contact from the server…")
                .color(theme::muted())
                .size(12.0),
        );
        return None;
    }

    if let Some(error) = &state.error {
        error_box(ui, error);
        ui.add_space(8.0);
    }

    let mut action = core_card(ui, state);
    ui.add_space(12.0);
    if let Some(next) = relationships_card(ui, state, catalog) {
        action = Some(next);
    }
    ui.add_space(12.0);
    profile_card(ui, state);
    action
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

fn core_card(ui: &mut Ui, state: &mut ContactWindowState) -> Option<ContactEditorAction> {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Stock fields").font(theme::strong_font(15.0)));
        ui.label(
            RichText::new("Core contact data stored by the server.")
                .color(theme::muted())
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
            role_summary(&mut cols[1], &state.role);
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

    None
}

fn profile_card(ui: &mut Ui, state: &mut ContactWindowState) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Profile details").font(theme::strong_font(15.0)));
        ui.label(
            RichText::new(
                "Profile data and organisation-defined fields are rendered together from the server.",
            )
            .color(theme::muted())
            .size(10.5),
        );
        ui.add_space(8.0);

        let keys: Vec<String> = state
            .details
            .keys()
            .filter(|key| {
                !matches!(
                    key.as_str(),
                    "contact_role" | "contact_status" | "relationships"
                )
            })
            .cloned()
            .collect();

        if keys.is_empty() && state.custom.is_empty() {
            ui.label(
                RichText::new("No additional profile data yet.")
                    .color(theme::faint())
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

fn relationships_card(
    ui: &mut Ui,
    state: &mut ContactWindowState,
    catalog: &[ApiGraphNode],
) -> Option<ContactEditorAction> {
    let mut action = None;

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Relationships").font(theme::strong_font(15.0)));
        ui.label(
            RichText::new(
                "Two-sided links stored by the server. Connect this record to any contact or organisation; new contacts can be created here and used immediately.",
            )
            .color(theme::muted())
            .size(10.5),
        );
        ui.add_space(8.0);

        if state.relationships_loading {
            ui.label(
                RichText::new("Loading relationships…")
                    .color(theme::muted())
                    .size(11.0),
            );
            ui.add_space(4.0);
        }
        if let Some(error) = &state.relationship_error {
            error_box(ui, error);
            ui.add_space(6.0);
        }

        if state.relationships.is_empty() && !state.relationships_loading {
            ui.label(
                RichText::new("No linked records yet.")
                    .color(theme::faint())
                    .size(11.0),
            );
            ui.add_space(6.0);
        }

        let mut remove: Option<String> = None;
        let mut open: Option<String> = None;
        for edge in &state.relationships {
            let Some(view) = relationship_view(edge, &state.contact_id) else {
                continue;
            };

            Frame::new()
                .fill(theme::surface_raised())
                .stroke(Stroke::new(1.0, theme::border()))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(relationship_role_label(&view.linked_role))
                                .font(theme::strong_font(12.5))
                                .color(theme::text()),
                        );
                        theme::pill(ui, view.linked_kind.label(), theme::accent_blue());
                        if view.linked_kind == PartyKind::Contact
                            && ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Open")
                                            .color(theme::accent())
                                            .size(10.5),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                        {
                            open = Some(view.linked_id.clone());
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let removing = state.relationship_removing.as_deref()
                                == Some(edge.id.as_str());
                            if ui
                                .add_enabled(
                                    !removing,
                                    egui::Button::new(
                                        RichText::new(if removing {
                                            "Removing…"
                                        } else {
                                            "Remove"
                                        })
                                        .color(theme::faint())
                                        .size(10.0),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                remove = Some(edge.id.clone());
                            }
                        });
                    });
                    ui.label(
                        RichText::new(&view.linked_name)
                            .color(theme::text())
                            .size(12.5),
                    );
                    ui.label(
                        RichText::new(format!(
                            "This record is {}",
                            relationship_role_label(&view.subject_role)
                        ))
                        .color(theme::faint())
                        .size(10.0),
                    );
                    if let Some(notes) = edge.notes.as_deref()
                        && !notes.trim().is_empty()
                    {
                        ui.label(
                            RichText::new(notes)
                                .color(theme::muted())
                                .size(10.5),
                        );
                    }
                });
            ui.add_space(6.0);
        }

        if let Some(id) = remove {
            state.relationship_removing = Some(id.clone());
            action = Some(ContactEditorAction::DeleteRelationship(id));
        }
        if let Some(id) = open {
            action = Some(ContactEditorAction::OpenContact(id));
        }

        ui.add_space(4.0);
        ui.separator();
        ui.add_space(8.0);
        ui.label(
            RichText::new("Add relationship")
                .font(theme::strong_font(12.5))
                .color(theme::text()),
        );

        let mut kind = state
            .relationship_kind
            .min(RELATIONSHIP_KINDS.len().saturating_sub(1));
        let kind_combo = egui::ComboBox::from_id_salt("relationship-kind")
            .selected_text(RELATIONSHIP_KINDS[kind].label)
            .width(ui.available_width().min(220.0))
            .show_ui(ui, |ui| {
                for (index, spec) in RELATIONSHIP_KINDS.iter().enumerate() {
                    ui.selectable_value(&mut kind, index, spec.label);
                }
            });
        named(&kind_combo.response, "Relationship type");
        state.relationship_kind = kind;
        let spec = &RELATIONSHIP_KINDS[kind];
        ui.label(
            RichText::new(format!(
                "This record is {} · the linked record is {}",
                relationship_role_label(spec.subject_role),
                relationship_role_label(spec.linked_role)
            ))
            .color(theme::faint())
            .size(10.0),
        );

        party_lookup(ui, state, catalog);

        ui.add(
            egui::TextEdit::singleline(&mut state.relationship_notes)
                .hint_text("Relationship notes (optional)")
                .desired_width(ui.available_width())
                .margin(Margin::symmetric(8, 6)),
        );

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let target = state.relationship_target.clone();
            let can_link = target.is_some() && !state.relationship_pending;
            if ui
                .add_enabled(can_link, theme::primary_button("Link relationship"))
                .clicked()
                && let Some(target) = target
            {
                state.relationship_pending = true;
                state.relationship_error = None;
                action = Some(ContactEditorAction::CreateRelationship {
                    from_role: spec.subject_role.to_owned(),
                    to_kind: target.kind,
                    to_id: target.id,
                    to_role: spec.linked_role.to_owned(),
                    notes: state.relationship_notes.trim().to_owned(),
                });
            }
            if ui.add(theme::secondary_button("+ New contact")).clicked() {
                state.new_relationship_contact = Some(NewRelationshipContact::new());
            }
            if state.relationship_pending {
                ui.label(
                    RichText::new("Saving…")
                        .color(theme::warning())
                        .size(10.5),
                );
            }
        });

        let mut cancelled = false;
        if let Some(mut draft) = state.new_relationship_contact.take() {
            let pending = state.relationship_pending;
            let relationship_error = state.relationship_error.clone();
            let from_role = spec.subject_role.to_owned();
            let to_role = spec.linked_role.to_owned();
            let notes = state.relationship_notes.trim().to_owned();

            ui.add_space(6.0);
            Frame::new()
                .fill(theme::input())
                .stroke(Stroke::new(1.0, theme::accent().gamma_multiply(0.5)))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new("New contact")
                            .font(theme::strong_font(12.5))
                            .color(theme::text()),
                    );
                    ui.add_space(4.0);
                    text_field(ui, "Display name", &mut draft.display_name);
                    ui.columns(2, |cols| {
                        text_field(&mut cols[0], "Given name", &mut draft.given_name);
                        text_field(&mut cols[1], "Family name", &mut draft.family_name);
                    });
                    ui.columns(2, |cols| {
                        text_field(&mut cols[0], "Email", &mut draft.email);
                        text_field(&mut cols[1], "Phone", &mut draft.phone);
                    });

                    if let Some(error) = &relationship_error {
                        ui.label(RichText::new(error).color(theme::danger()).size(10.5));
                    }

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let valid = !draft.display_name.trim().is_empty();
                        if ui
                            .add_enabled(
                                valid && !pending,
                                theme::primary_button("Create & link"),
                            )
                            .clicked()
                        {
                            state.relationship_pending = true;
                            state.relationship_error = None;
                            action = Some(ContactEditorAction::CreateContact {
                                from_role: from_role.clone(),
                                to_role: to_role.clone(),
                                notes: notes.clone(),
                                draft: draft.clone(),
                            });
                        }
                        if ui.add(theme::secondary_button("Cancel")).clicked() {
                            cancelled = true;
                            state.relationship_error = None;
                        }
                    });
                });
            if !cancelled {
                state.new_relationship_contact = Some(draft);
            }
        }
    });

    action
}

struct RelationshipView {
    subject_role: String,
    linked_kind: PartyKind,
    linked_id: String,
    linked_name: String,
    linked_role: String,
}

fn relationship_view(edge: &ApiRelationship, contact_id: &str) -> Option<RelationshipView> {
    if edge.from_kind == PartyKind::Contact && edge.from_id == contact_id {
        Some(RelationshipView {
            subject_role: edge.from_role.clone(),
            linked_kind: edge.to_kind,
            linked_id: edge.to_id.clone(),
            linked_name: edge.to_name.clone(),
            linked_role: edge.to_role.clone(),
        })
    } else if edge.to_kind == PartyKind::Contact && edge.to_id == contact_id {
        Some(RelationshipView {
            subject_role: edge.to_role.clone(),
            linked_kind: edge.from_kind,
            linked_id: edge.from_id.clone(),
            linked_name: edge.from_name.clone(),
            linked_role: edge.from_role.clone(),
        })
    } else {
        None
    }
}

fn party_lookup(ui: &mut Ui, state: &mut ContactWindowState, catalog: &[ApiGraphNode]) {
    let search_id = ui.scope_id().with("relationship-party-search");
    let mut search = ui.data_mut(|data| data.get_temp::<String>(search_id).unwrap_or_default());
    let selected = state
        .relationship_target
        .as_ref()
        .map(|node| format!("{}  ·  {}", node.label, node.kind.label()))
        .unwrap_or_else(|| "Search contacts and organisations…".to_owned());
    let mut chosen: Option<ApiGraphNode> = None;

    let combo = egui::ComboBox::from_id_salt("relationship-party")
        .selected_text(selected)
        .width(ui.available_width().max(150.0))
        .show_ui(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut search)
                    .hint_text("Type a name…")
                    .desired_width(260.0)
                    .margin(Margin::symmetric(8, 6)),
            );
            let needle = search.trim().to_lowercase();
            ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                if catalog.is_empty() {
                    ui.label(
                        RichText::new("Loading the contact and organisation directory…")
                            .color(theme::faint())
                            .size(10.5),
                    );
                    return;
                }

                let mut shown = 0;
                for node in catalog
                    .iter()
                    .filter(|node| {
                        !(node.kind == PartyKind::Contact && node.id == state.contact_id)
                    })
                    .filter(|node| needle.is_empty() || node.label.to_lowercase().contains(&needle))
                {
                    if shown >= 60 {
                        break;
                    }
                    shown += 1;
                    let current = state
                        .relationship_target
                        .as_ref()
                        .is_some_and(|target| target.kind == node.kind && target.id == node.id);
                    if ui
                        .selectable_label(
                            current,
                            format!("{}  ·  {}", node.label, node.kind.label()),
                        )
                        .clicked()
                    {
                        chosen = Some(node.clone());
                        ui.close();
                    }
                }
                if shown == 0 {
                    ui.label(
                        RichText::new("No matching records")
                            .color(theme::faint())
                            .size(10.5),
                    );
                }
            });
        });
    named(&combo.response, "Linked record");

    ui.data_mut(|data| data.insert_temp(search_id, search));
    if let Some(node) = chosen {
        state.relationship_target = Some(node);
    }
}

fn render_detail_value(ui: &mut Ui, key: &str, value: &mut Value, depth: usize) {
    match value {
        Value::Object(map) => {
            ui.label(
                RichText::new(label_for(key))
                    .color(theme::muted())
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
        ui.label(RichText::new(help).color(theme::faint()).size(9.5));
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

/// Read-only role. It is derived from relationships with the organisation, so
/// it changes by adding or ending a Care Provider or Employer relationship.
fn role_summary(ui: &mut Ui, role: &str) {
    ui.horizontal(|ui| {
        role_pill(ui, role);
    });
    ui.label(
        RichText::new(
            "Set by relationships: add a Care Provider or Employer link to the organisation.",
        )
        .color(theme::faint())
        .size(10.0),
    );
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
        "client" => theme::accent_blue(),
        "worker" => theme::accent(),
        "dual" => theme::warning(),
        _ => theme::faint(),
    };
    theme::pill(ui, &role_label(role).to_uppercase(), colour);
}

fn status_pill(ui: &mut Ui, status: &str) {
    let colour = match status {
        "Active" => theme::success(),
        "Onboarding" => theme::warning(),
        _ => theme::faint(),
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
        ui.label(RichText::new(label).color(theme::muted()).size(10.5));
        if required {
            ui.label(RichText::new("*").color(theme::accent()).size(10.5));
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
            theme::danger().r(),
            theme::danger().g(),
            theme::danger().b(),
            20,
        ))
        .stroke(Stroke::new(1.0, theme::danger().gamma_multiply(0.55)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(message).color(theme::danger()).size(11.0));
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

#[cfg(test)]
mod tests {
    use super::{ContactWindowState, relationship_view};
    use crate::contact::{ApiRelationship, PartyKind};

    fn edge() -> ApiRelationship {
        ApiRelationship {
            id: "rel-1".to_owned(),
            from_kind: PartyKind::Contact,
            from_id: "jamie".to_owned(),
            from_name: "Jamie Lee".to_owned(),
            from_role: "person_supported".to_owned(),
            to_kind: PartyKind::Organisation,
            to_id: "clinic".to_owned(),
            to_name: "Northside Medical Centre".to_owned(),
            to_role: "general_practitioner".to_owned(),
            start_date: None,
            end_date: None,
            notes: None,
            updated_at: String::new(),
        }
    }

    #[test]
    fn reads_relationships_from_either_endpoint() {
        let edge = edge();
        let from = relationship_view(&edge, "jamie").unwrap();
        assert_eq!(from.subject_role, "person_supported");
        assert_eq!(from.linked_kind, PartyKind::Organisation);
        assert_eq!(from.linked_name, "Northside Medical Centre");
        assert_eq!(from.linked_role, "general_practitioner");

        let to = relationship_view(&edge, "clinic");
        assert!(to.is_none(), "the clinic is not a contact record");

        let mut reversed = edge;
        reversed.from_kind = PartyKind::Contact;
        reversed.from_id = "alex".to_owned();
        reversed.from_name = "Alex Chen".to_owned();
        reversed.from_role = "emergency_contact".to_owned();
        reversed.to_kind = PartyKind::Contact;
        reversed.to_id = "jamie".to_owned();
        reversed.to_name = "Jamie Lee".to_owned();
        reversed.to_role = "person_supported".to_owned();

        let view = relationship_view(&reversed, "jamie").unwrap();
        assert_eq!(view.subject_role, "person_supported");
        assert_eq!(view.linked_id, "alex");
        assert_eq!(view.linked_role, "emergency_contact");
    }

    #[test]
    fn relationships_are_not_written_to_contact_details() {
        let mut state = ContactWindowState::new("jamie".to_owned());
        state.display_name = "Jamie Lee".to_owned();
        state.relationships.push(edge());

        let write = state.build_write().unwrap();
        assert!(write.details.get("relationships").is_none());
    }
}
