//! Relationship network: create and manage two-sided links between any two
//! records, whether contacts or organisations.

use eframe::egui::{
    self, Align, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea, Stroke, Ui, vec2,
};

use crate::{
    contact::{ApiGraphNode, ApiRelationship, PartyKind, RoleOption, relationship_role_label},
    pages::contact_editor::role_picker,
    theme,
};

#[derive(Default)]
pub struct RelationshipsForm {
    pub from: Option<ApiGraphNode>,
    pub to: Option<ApiGraphNode>,
    /// The relationship type: `subject_role` is the From side's role.
    pub role: Option<RoleOption>,
    pub notes: String,
    pub filter: String,
    pub error: Option<String>,
    pub pending: bool,
}

impl RelationshipsForm {
    pub fn clear_selection(&mut self) {
        self.from = None;
        self.to = None;
        self.notes.clear();
        self.error = None;
        self.pending = false;
    }
}

pub enum RelationshipsAction {
    Create {
        from_kind: PartyKind,
        from_id: String,
        from_role: String,
        to_kind: PartyKind,
        to_id: String,
        to_role: String,
        notes: String,
    },
    OpenContact(String),
    Refresh,
}

pub fn show(
    ui: &mut Ui,
    form: &mut RelationshipsForm,
    nodes: &[ApiGraphNode],
    edges: &[ApiRelationship],
    roles: &[RoleOption],
    loading: bool,
) -> Option<RelationshipsAction> {
    let mut action = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            header(ui, edges.len(), loading, &mut action);
            ui.add_space(18.0);

            add_card(ui, form, nodes, roles, &mut action);
            ui.add_space(14.0);
            list_card(ui, form, edges, loading, &mut action);
            ui.add_space(24.0);
        });

    action
}

fn header(ui: &mut Ui, total: usize, loading: bool, action: &mut Option<RelationshipsAction>) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(52.0, 52.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, CornerRadius::same(14), theme::surface_raised());
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "🔗",
            egui::FontId::proportional(22.0),
            theme::text(),
        );
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(RichText::new("Relationships").font(theme::strong_font(26.0)));
            ui.label(
                RichText::new(
                    "Two-sided links between any contacts and organisations in the network.",
                )
                .color(theme::muted())
                .size(13.0),
            );
        });

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.add(theme::secondary_button("Refresh network")).clicked() {
                *action = Some(RelationshipsAction::Refresh);
            }
            if loading {
                ui.label(RichText::new("Loading…").color(theme::warning()).size(11.0));
            } else {
                theme::pill(ui, &format!("{total} LINKS"), theme::accent());
            }
        });
    });
}

fn add_card(
    ui: &mut Ui,
    form: &mut RelationshipsForm,
    nodes: &[ApiGraphNode],
    roles: &[RoleOption],
    action: &mut Option<RelationshipsAction>,
) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Add relationship").font(theme::strong_font(15.0)));
        ui.label(
            RichText::new(
                "Pick both ends and the role each side holds. Contacts and organisations can be linked in any combination.",
            )
            .color(theme::muted())
            .size(11.0),
        );
        ui.add_space(10.0);

        if nodes.is_empty() {
            ui.label(
                RichText::new("Loading the contact and organisation directory…")
                    .color(theme::faint())
                    .size(11.0),
            );
            return;
        }

        ui.columns(2, |cols| {
            cols[0].label(
                RichText::new("From")
                    .color(theme::muted())
                    .font(theme::strong_font(11.0)),
            );
            party_picker(
                &mut cols[0],
                "relationship-from",
                &mut form.from,
                nodes,
                form.to.as_ref().map(|node| (node.kind, node.id.as_str())),
            );

            cols[1].label(
                RichText::new("To")
                    .color(theme::muted())
                    .font(theme::strong_font(11.0)),
            );
            party_picker(
                &mut cols[1],
                "relationship-to",
                &mut form.to,
                nodes,
                form.from.as_ref().map(|node| (node.kind, node.id.as_str())),
            );
        });

        ui.add_space(6.0);
        role_picker(ui, "relationship-page-role", roles, &mut form.role, |role| {
            format!(
                "From is {} · To is {}",
                relationship_role_label(&role.subject_role),
                relationship_role_label(&role.linked_role)
            )
        });
        ui.label(
            RichText::new(
                "Roles come from your organisation's relationship types. An administrator can add more.",
            )
            .color(theme::faint())
            .size(10.0),
        );
        ui.add_space(4.0);
        text_field(ui, "Notes (optional)", &mut form.notes);

        if let Some(error) = &form.error {
            ui.add_space(4.0);
            ui.label(RichText::new(error).color(theme::danger()).size(10.5));
        }

        ui.add_space(6.0);
        let ready = form.from.is_some()
            && form.to.is_some()
            && form.role.is_some()
            && !form.pending;
        if ui
            .add_enabled(ready, theme::primary_button("Link records"))
            .clicked()
            && let (Some(from), Some(to), Some(role)) =
                (form.from.clone(), form.to.clone(), form.role.clone())
        {
            form.pending = true;
            form.error = None;
            *action = Some(RelationshipsAction::Create {
                from_kind: from.kind,
                from_id: from.id,
                from_role: role.subject_role,
                to_kind: to.kind,
                to_id: to.id,
                to_role: role.linked_role,
                notes: form.notes.trim().to_owned(),
            });
        }
    });
}

fn list_card(
    ui: &mut Ui,
    form: &mut RelationshipsForm,
    edges: &[ApiRelationship],
    loading: bool,
    action: &mut Option<RelationshipsAction>,
) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Network").font(theme::strong_font(15.0)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let filter = ui.add(
                    egui::TextEdit::singleline(&mut form.filter)
                        .hint_text("Filter by name or role…")
                        .desired_width(260.0)
                        .margin(Margin::symmetric(8, 6)),
                );
                filter.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Filter relationships")
                });
            });
        });

        let needle = form.filter.trim().to_lowercase();
        let mut shown = 0usize;
        let mut manage: Option<String> = None;
        let mut open = None;

        if edges.is_empty() && !loading {
            ui.add_space(6.0);
            ui.label(
                RichText::new("No relationships have been recorded yet.")
                    .color(theme::faint())
                    .size(11.0),
            );
        }

        for edge in edges {
            let haystack = format!(
                "{} {} {} {}",
                edge.from_name, edge.from_role, edge.to_name, edge.to_role
            )
            .to_lowercase();
            if !needle.is_empty() && !haystack.contains(&needle) {
                continue;
            }
            shown += 1;
            if shown > 200 {
                break;
            }

            Frame::new()
                .fill(theme::surface_raised())
                .stroke(Stroke::new(1.0, theme::border()))
                .corner_radius(CornerRadius::same(9))
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        endpoint(
                            ui,
                            edge.from_name.as_str(),
                            edge.from_kind,
                            relationship_role_label(&edge.from_role).as_str(),
                            edge.from_kind == PartyKind::Contact,
                            &mut open,
                            &edge.from_id,
                        );
                        ui.label(RichText::new("↔").color(theme::faint()).size(13.0));
                        endpoint(
                            ui,
                            edge.to_name.as_str(),
                            edge.to_kind,
                            relationship_role_label(&edge.to_role).as_str(),
                            edge.to_kind == PartyKind::Contact,
                            &mut open,
                            &edge.to_id,
                        );

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            // End, transfer, correct and remove are done from a
                            // contact, where the relationship's history is shown.
                            let contact = if edge.from_kind == PartyKind::Contact {
                                Some(&edge.from_id)
                            } else if edge.to_kind == PartyKind::Contact {
                                Some(&edge.to_id)
                            } else {
                                None
                            };
                            if let Some(contact) = contact
                                && ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new("Manage…")
                                                .color(theme::accent())
                                                .size(10.0),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_text(
                                        "End, transfer, correct or remove from the contact",
                                    )
                                    .clicked()
                            {
                                manage = Some(contact.clone());
                            }
                        });
                    });
                    if let Some(notes) = edge.notes.as_deref()
                        && !notes.trim().is_empty()
                    {
                        ui.label(RichText::new(notes).color(theme::muted()).size(10.5));
                    }
                });
            ui.add_space(5.0);
        }

        if shown == 0 && !edges.is_empty() {
            ui.label(
                RichText::new("No relationships match that filter.")
                    .color(theme::faint())
                    .size(10.5),
            );
        }

        if let Some(id) = manage {
            *action = Some(RelationshipsAction::OpenContact(id));
        }
        if let Some(id) = open {
            *action = Some(RelationshipsAction::OpenContact(id));
        }
    });
}

fn endpoint(
    ui: &mut Ui,
    name: &str,
    kind: PartyKind,
    role: &str,
    can_open: bool,
    open: &mut Option<String>,
    id: &str,
) {
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(name)
                    .color(theme::text())
                    .font(theme::strong_font(12.0)),
            );
            theme::pill(ui, kind.label(), theme::accent_blue());
            if can_open
                && ui
                    .add(
                        egui::Button::new(RichText::new("Open").color(theme::accent()).size(10.0))
                            .frame(false),
                    )
                    .clicked()
            {
                *open = Some(id.to_owned());
            }
        });
        ui.label(RichText::new(role).color(theme::faint()).size(10.0));
    });
}

fn party_picker(
    ui: &mut Ui,
    id: &str,
    selected: &mut Option<ApiGraphNode>,
    nodes: &[ApiGraphNode],
    exclude: Option<(PartyKind, &str)>,
) {
    let search_id = ui.scope_id().with(("party-search", id));
    let mut search = ui.data_mut(|data| data.get_temp::<String>(search_id).unwrap_or_default());
    let label = selected
        .as_ref()
        .map(|node| format!("{} · {}", node.label, node.kind.label()))
        .unwrap_or_else(|| "Select a record…".to_owned());
    let mut chosen: Option<ApiGraphNode> = None;

    let combo = egui::ComboBox::from_id_salt(id)
        .selected_text(label)
        .width(ui.available_width())
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut search)
                    .hint_text("Search contacts and organisations…")
                    .desired_width(260.0)
                    .margin(Margin::symmetric(8, 6)),
            );
            let needle = search.trim().to_lowercase();
            ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                let mut shown = 0;
                for node in nodes.iter().filter(|node| {
                    !exclude.is_some_and(|(kind, id)| node.kind == kind && node.id == id)
                }) {
                    if !needle.is_empty() && !node.label.to_lowercase().contains(&needle) {
                        continue;
                    }
                    if shown >= 60 {
                        break;
                    }
                    shown += 1;
                    let current = selected
                        .as_ref()
                        .is_some_and(|value| value.kind == node.kind && value.id == node.id);
                    if ui
                        .selectable_label(
                            current,
                            format!("{} · {}", node.label, node.kind.label()),
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
    let response = combo.response;
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Relationship party")
    });

    ui.data_mut(|data| data.insert_temp(search_id, search));
    if let Some(node) = chosen {
        *selected = Some(node);
    }
}

fn text_field(ui: &mut Ui, label: &str, value: &mut String) {
    ui.add_space(4.0);
    ui.label(
        RichText::new(label)
            .color(theme::muted())
            .font(theme::strong_font(11.0)),
    );
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .desired_width(ui.available_width())
            .margin(Margin::symmetric(9, 7)),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
}

fn _unused(_: &mut Ui) {}
