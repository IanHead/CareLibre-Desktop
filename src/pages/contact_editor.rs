//! Native, free-floating contact editor windows backed by the server.

use std::collections::BTreeMap;

use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea, Stroke, Ui,
    vec2,
};
use serde_json::{Map, Value};

use crate::contact::{
    ApiAgreement, ApiAgreementLine, ApiAgreementLineWrite, ApiAgreementWrite, ApiClientRequirement,
    ApiClientRequirementWrite, ApiContactDetail, ApiContactWrite, ApiCustomField,
    ApiFundingPackage, ApiFundingPackageWrite, ApiFundingService, ApiFundingSource, ApiGraphNode,
    ApiRelationship, ApiRelationshipCorrection, ApiRelationshipEvent, ApiWorkerQuality,
    ApiWorkerQualityValue, ApiWorkerQualityValueWrite, END_REASONS, PartyKind, RelationshipChange,
    RoleOption, agreement_status_label, category_label, claiming_model_label, end_reason_label,
    funding_kind_label, history_action_label, quality_value_label, relationship_role_label,
};
use crate::theme;

/// The contact and organisation roles that make a contact a client: the
/// organisation provides care to them.
pub const CLIENT_ROLES: (&str, &str) = ("care_recipient", "provider");

/// The contact and organisation roles that make a contact a worker.
pub const WORKER_ROLES: (&str, &str) = ("employee", "employer");

/// Draw a searchable list of relationship roles from the catalogue, grouped by
/// category. `describe` turns an option into its text. Returns true when the
/// selection changed.
pub fn role_picker(
    ui: &mut Ui,
    id: &str,
    options: &[RoleOption],
    selected: &mut Option<RoleOption>,
    describe: impl Fn(&RoleOption) -> String,
) -> bool {
    let mut changed = false;
    let search_id = ui.make_persistent_id((id, "search"));
    let text = selected
        .as_ref()
        .map_or_else(|| "Choose a relationship…".to_owned(), &describe);
    let combo = egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .width(ui.available_width().min(340.0))
        .height(340.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            if options.is_empty() {
                ui.label(
                    RichText::new("Loading relationship types…")
                        .color(theme::muted())
                        .size(10.5),
                );
                return;
            }
            let mut search =
                ui.data_mut(|data| data.get_temp::<String>(search_id).unwrap_or_default());
            let field = ui.add(
                egui::TextEdit::singleline(&mut search)
                    .hint_text("Filter…")
                    .desired_width(ui.available_width()),
            );
            named(&field, "Filter relationship types");
            ui.data_mut(|data| data.insert_temp(search_id, search.clone()));

            let mut category = "";
            let mut shown = 0;
            for option in options.iter().filter(|option| option.matches(&search)) {
                if option.category != category {
                    category = &option.category;
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(category_label(category))
                            .color(theme::faint())
                            .size(9.5),
                    );
                }
                let is_selected = selected.as_ref() == Some(option);
                if ui.selectable_label(is_selected, describe(option)).clicked() {
                    *selected = Some(option.clone());
                    changed = true;
                    ui.data_mut(|data| data.remove::<String>(search_id));
                    ui.close();
                }
                shown += 1;
            }
            if shown == 0 {
                ui.label(
                    RichText::new("No relationship types match. An administrator can add one.")
                        .color(theme::faint())
                        .size(10.0),
                );
            }
        });
    named(&combo.response, "Relationship type");
    changed
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewRelationshipContact {
    pub given_name: String,
    pub family_name: String,
    pub email: String,
    pub phone: String,
}

impl NewRelationshipContact {
    pub(crate) fn new() -> Self {
        Self {
            given_name: String::new(),
            family_name: String::new(),
            email: String::new(),
            phone: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
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
    /// End, transfer, change role, correct or remove a relationship.
    ChangeRelationship {
        id: String,
        change: RelationshipChange,
    },
    LoadRelationshipHistory(String),
    CreateContact {
        from_role: String,
        to_role: String,
        notes: String,
        draft: NewRelationshipContact,
    },
    /// Save a skill or training the worker holds.
    SetQualityValue(ApiWorkerQualityValueWrite),
    /// Remove a skill or training by quality id.
    DeleteQualityValue(String),
    /// Save a requirement the client places on workers.
    SetRequirement(ApiClientRequirementWrite),
    /// Remove a requirement by id.
    DeleteRequirement(String),
    /// Add a funding package to a client.
    AddFundingPackage(ApiFundingPackageWrite),
    /// Add a quote/service agreement to a client.
    AddAgreement(ApiAgreementWrite),
    /// Add a line to an agreement.
    AddAgreementLine {
        agreement_id: String,
        payload: ApiAgreementLineWrite,
    },
    /// Remove a line from an agreement.
    DeleteAgreementLine {
        agreement_id: String,
        line_id: String,
    },
    /// Finalise a quote and generate draft shifts.
    FinaliseAgreement(String),
}

/// Lifecycle actions offered on a relationship.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelationshipFormKind {
    End,
    Transfer,
    ChangeRole,
    Correct,
    Remove,
}

impl RelationshipFormKind {
    fn title(self) -> &'static str {
        match self {
            Self::End => "End relationship",
            Self::Transfer => "Transfer to someone else",
            Self::ChangeRole => "Change role",
            Self::Correct => "Correct a mistake",
            Self::Remove => "Remove (recorded in error)",
        }
    }

    fn help(self) -> &'static str {
        match self {
            Self::End => {
                "The relationship stays in the history. The end date can be in the future, such as the end of a notice period."
            }
            Self::Transfer => {
                "Another party takes over from the date below. The current relationship ends the day before and the history links the two."
            }
            Self::ChangeRole => {
                "The relationship continues in new roles from the date below, for example family member to guardian."
            }
            Self::Correct => {
                "Fix details that were recorded wrongly. This is saved as a correction, not as a change in circumstances."
            }
            Self::Remove => {
                "Only for a relationship that was never true. It disappears everywhere but stays in the audit trail."
            }
        }
    }
}

/// A lifecycle form open against one relationship.
#[derive(Clone, Debug)]
pub struct RelationshipForm {
    pub edge_id: String,
    pub kind: RelationshipFormKind,
    /// End date (End) or first day of the new arrangement (Transfer, Change role).
    pub date: String,
    /// Index into [`END_REASONS`]. Always chosen explicitly.
    pub reason: Option<usize>,
    pub note: String,
    /// New roles for Change role, or the corrected roles for Correct, read
    /// from this record's side.
    pub role: Option<RoleOption>,
    // Correct
    pub from_role: String,
    pub to_role: String,
    pub start_date: String,
    pub end_date: String,
    pub notes: String,
    pub pending: bool,
    pub error: Option<String>,
}

impl RelationshipForm {
    fn new(edge: &ApiRelationship, kind: RelationshipFormKind) -> Self {
        Self {
            edge_id: edge.id.clone(),
            kind,
            date: chrono::Local::now().date_naive().to_string(),
            reason: if kind == RelationshipFormKind::Correct {
                edge.end_reason
                    .as_deref()
                    .and_then(|key| END_REASONS.iter().position(|(value, _)| *value == key))
            } else {
                None
            },
            note: String::new(),
            role: None,
            from_role: edge.from_role.clone(),
            to_role: edge.to_role.clone(),
            start_date: edge.start_date.clone().unwrap_or_default(),
            end_date: edge.end_date.clone().unwrap_or_default(),
            notes: edge.notes.clone().unwrap_or_default(),
            pending: false,
            error: None,
        }
    }
}

/// History shown under one relationship.
#[derive(Clone, Debug)]
pub struct RelationshipHistoryView {
    pub edge_id: String,
    pub loading: bool,
    /// Set after a change so the history reloads.
    pub stale: bool,
    pub items: Vec<ApiRelationshipEvent>,
    pub error: Option<String>,
}

pub struct ContactWindowState {
    pub contact_id: String,
    pub request_seq: u64,
    pub request_id: Option<u64>,
    pub relationship_request_id: Option<u64>,
    pub loading: bool,
    pub saving: bool,
    pub detail: Option<ApiContactDetail>,
    pub given_name: String,
    pub family_name: String,
    pub preferred_name: String,
    pub pronouns: String,
    pub date_of_birth: String,
    pub gender: String,
    pub languages: String,
    pub cultural_identity: String,
    pub email: String,
    pub phone: String,
    pub notes: String,
    pub role: String,
    pub status: String,
    pub details: Map<String, Value>,
    pub custom: BTreeMap<String, String>,
    pub quality_values: Vec<ApiWorkerQualityValue>,
    pub requirements: Vec<ApiClientRequirement>,
    pub quality_error: Option<String>,
    pub requirement_error: Option<String>,
    pub quality_pending: bool,
    pub requirement_pending: bool,
    pub new_quality_id: Option<String>,
    pub new_quality_held: bool,
    pub new_quality_value: String,
    pub new_quality_expires: String,
    pub new_quality_evidence: String,
    pub new_requirement_quality_id: Option<String>,
    pub new_requirement_operator: String,
    pub new_requirement_value: String,
    pub new_requirement_held: bool,
    pub new_requirement_severity: String,
    pub new_requirement_reason: String,
    pub funding_sources: Vec<ApiFundingSource>,
    pub funding_services: Vec<ApiFundingService>,
    pub funding_packages: Vec<ApiFundingPackage>,
    pub agreements: Vec<ApiAgreement>,
    pub funding_error: Option<String>,
    pub funding_notice: Option<String>,
    pub funding_pending: bool,
    pub new_package_source_id: Option<String>,
    pub new_package_name: String,
    pub new_package_reference: String,
    pub new_package_start: String,
    pub new_package_end: String,
    pub new_package_budget: String,
    pub new_agreement_package_id: Option<String>,
    pub new_agreement_title: String,
    pub new_agreement_start: String,
    pub new_agreement_end: String,
    pub new_line_agreement_id: Option<String>,
    pub new_line_service_id: Option<String>,
    pub new_line_description: String,
    pub new_line_quantity: String,
    pub new_line_unit: String,
    pub new_line_rate: String,
    pub new_line_recurrence: String,
    pub new_line_weekdays: Vec<i16>,
    pub new_line_start_time: String,
    pub new_line_duration: String,
    pub relationships: Vec<ApiRelationship>,
    pub relationships_loading: bool,
    /// The relationship type chosen in the Add form, from this record's side.
    pub relationship_role: Option<RoleOption>,
    pub relationship_target: Option<ApiGraphNode>,
    pub relationship_notes: String,
    pub relationship_pending: bool,
    /// The lifecycle form open against one relationship, if any.
    pub relationship_form: Option<RelationshipForm>,
    /// The history shown under one relationship, if any.
    pub relationship_history: Option<RelationshipHistoryView>,
    pub show_ended_relationships: bool,
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
            given_name: String::new(),
            family_name: String::new(),
            preferred_name: String::new(),
            pronouns: String::new(),
            date_of_birth: String::new(),
            gender: String::new(),
            languages: String::new(),
            cultural_identity: String::new(),
            email: String::new(),
            phone: String::new(),
            notes: String::new(),
            role: "contact".to_owned(),
            status: "Active".to_owned(),
            details: Map::new(),
            custom: BTreeMap::new(),
            quality_values: Vec::new(),
            requirements: Vec::new(),
            quality_error: None,
            requirement_error: None,
            quality_pending: false,
            requirement_pending: false,
            new_quality_id: None,
            new_quality_held: true,
            new_quality_value: String::new(),
            new_quality_expires: String::new(),
            new_quality_evidence: String::new(),
            new_requirement_quality_id: None,
            new_requirement_operator: "exists".to_owned(),
            new_requirement_value: String::new(),
            new_requirement_held: true,
            new_requirement_severity: "warn".to_owned(),
            new_requirement_reason: String::new(),
            funding_sources: Vec::new(),
            funding_services: Vec::new(),
            funding_packages: Vec::new(),
            agreements: Vec::new(),
            funding_error: None,
            funding_notice: None,
            funding_pending: false,
            new_package_source_id: None,
            new_package_name: String::new(),
            new_package_reference: String::new(),
            new_package_start: String::new(),
            new_package_end: String::new(),
            new_package_budget: String::new(),
            new_agreement_package_id: None,
            new_agreement_title: String::new(),
            new_agreement_start: String::new(),
            new_agreement_end: String::new(),
            new_line_agreement_id: None,
            new_line_service_id: None,
            new_line_description: String::new(),
            new_line_quantity: "1".to_owned(),
            new_line_unit: "hour".to_owned(),
            new_line_rate: String::new(),
            new_line_recurrence: "weekly".to_owned(),
            new_line_weekdays: vec![1, 3],
            new_line_start_time: "09:00".to_owned(),
            new_line_duration: "120".to_owned(),
            relationships: Vec::new(),
            relationships_loading: false,
            relationship_role: None,
            relationship_target: None,
            relationship_notes: String::new(),
            relationship_pending: false,
            relationship_form: None,
            relationship_history: None,
            show_ended_relationships: false,
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
        self.given_name = contact.given_name.clone().unwrap_or_default();
        self.family_name = contact.family_name.clone().unwrap_or_default();
        self.preferred_name = contact.preferred_name.clone().unwrap_or_default();
        self.pronouns = contact.pronouns.clone().unwrap_or_default();
        self.date_of_birth = contact.date_of_birth.clone().unwrap_or_default();
        self.gender = contact.gender.clone().unwrap_or_default();
        self.languages = contact.languages.join(", ");
        self.cultural_identity = contact.cultural_identity.clone().unwrap_or_default();
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
        self.relationship_form = None;
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
        self.quality_values = detail.quality_values.clone();
        self.requirements = detail.requirements.clone();
        self.funding_sources = detail.funding_sources.clone();
        self.funding_services = detail.funding_services.clone();
        self.funding_packages = detail.funding_packages.clone();
        self.agreements = detail.agreements.clone();
        self.funding_error = None;
        self.funding_notice = None;
        self.funding_pending = false;
        self.quality_pending = false;
        self.requirement_pending = false;
        self.quality_error = None;
        self.requirement_error = None;
        self.detail = Some(detail);
        self.loading = false;
        self.saving = false;
        self.error = None;
        self.close_requested = false;
    }

    pub fn computed_display_name(&self) -> String {
        let preferred = self.preferred_name.trim();
        let given = self.given_name.trim();
        let family = self.family_name.trim();
        let first = if preferred.is_empty() {
            given
        } else {
            preferred
        };
        match (first.is_empty(), family.is_empty()) {
            (false, false) => format!("{first} {family}"),
            (false, true) => first.to_owned(),
            (true, false) => family.to_owned(),
            (true, true) => String::new(),
        }
    }

    pub fn title(&self) -> String {
        let name = self.computed_display_name();
        if name.is_empty() {
            "Contact — CareLibre".to_owned()
        } else {
            format!("{name} — CareLibre")
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
        self.relationship_target = None;
        self.relationship_notes.clear();
        self.new_relationship_contact = None;
        self.relationship_error = None;
    }

    pub fn relationship_failed(&mut self, message: String) {
        self.relationship_pending = false;
        self.relationship_error = Some(message);
    }

    /// A lifecycle change was saved; the caller reloads the relationships.
    pub fn relationship_changed(&mut self) {
        self.relationship_form = None;
        self.relationship_target = None;
        if let Some(history) = &mut self.relationship_history {
            history.stale = true;
        }
    }

    pub fn relationship_change_failed(&mut self, message: String) {
        match &mut self.relationship_form {
            Some(form) => {
                form.pending = false;
                form.error = Some(message);
            }
            None => self.relationship_error = Some(message),
        }
    }

    pub fn history_loaded(&mut self, id: &str, result: Result<Vec<ApiRelationshipEvent>, String>) {
        if let Some(history) = &mut self.relationship_history
            && history.edge_id == id
        {
            history.loading = false;
            match result {
                Ok(items) => {
                    history.items = items;
                    history.error = None;
                }
                Err(error) => history.error = Some(error),
            }
        }
    }

    pub fn quality_value_saved(&mut self, value: ApiWorkerQualityValue) {
        if let Some(existing) = self
            .quality_values
            .iter_mut()
            .find(|existing| existing.quality_id == value.quality_id)
        {
            *existing = value;
        } else {
            self.quality_values.push(value);
        }
        self.quality_values
            .sort_by(|a, b| a.quality_name.cmp(&b.quality_name));
        self.quality_pending = false;
        self.quality_error = None;
        self.new_quality_id = None;
        self.new_quality_held = true;
        self.new_quality_value.clear();
        self.new_quality_expires.clear();
        self.new_quality_evidence.clear();
    }

    pub fn quality_value_failed(&mut self, message: String) {
        self.quality_pending = false;
        self.quality_error = Some(message);
    }

    pub fn quality_value_deleted(&mut self, quality_id: &str) {
        self.quality_values
            .retain(|value| value.quality_id != quality_id);
        self.quality_pending = false;
        self.quality_error = None;
    }

    pub fn requirement_saved(&mut self, requirement: ApiClientRequirement) {
        if let Some(existing) = self
            .requirements
            .iter_mut()
            .find(|existing| existing.id == requirement.id)
        {
            *existing = requirement;
        } else {
            self.requirements.push(requirement);
        }
        self.requirements
            .sort_by(|a, b| a.quality_name.cmp(&b.quality_name));
        self.requirement_pending = false;
        self.requirement_error = None;
        self.new_requirement_quality_id = None;
        self.new_requirement_operator = "exists".to_owned();
        self.new_requirement_value.clear();
        self.new_requirement_held = true;
        self.new_requirement_severity = "warn".to_owned();
        self.new_requirement_reason.clear();
    }

    pub fn requirement_failed(&mut self, message: String) {
        self.requirement_pending = false;
        self.requirement_error = Some(message);
    }

    pub fn requirement_deleted(&mut self, requirement_id: &str) {
        self.requirements
            .retain(|requirement| requirement.id != requirement_id);
        self.requirement_pending = false;
        self.requirement_error = None;
    }

    pub fn funding_package_saved(&mut self, package: ApiFundingPackage) {
        if let Some(existing) = self
            .funding_packages
            .iter_mut()
            .find(|existing| existing.id == package.id)
        {
            *existing = package;
        } else {
            self.funding_packages.push(package);
        }
        self.funding_packages
            .sort_by(|a, b| b.start_date.cmp(&a.start_date));
        self.funding_pending = false;
        self.funding_error = None;
        self.new_package_source_id = None;
        self.new_package_name.clear();
        self.new_package_reference.clear();
        self.new_package_start.clear();
        self.new_package_end.clear();
        self.new_package_budget.clear();
    }

    pub fn agreement_saved(&mut self, agreement: ApiAgreement) {
        if let Some(existing) = self
            .agreements
            .iter_mut()
            .find(|existing| existing.id == agreement.id)
        {
            *existing = agreement;
        } else {
            self.agreements.push(agreement);
        }
        self.agreements
            .sort_by(|a, b| b.start_date.cmp(&a.start_date));
        self.funding_pending = false;
        self.funding_error = None;
        self.new_agreement_package_id = None;
        self.new_agreement_title.clear();
        self.new_agreement_start.clear();
        self.new_agreement_end.clear();
    }

    pub fn agreement_line_saved(&mut self, agreement_id: &str, line: ApiAgreementLine) {
        if let Some(agreement) = self
            .agreements
            .iter_mut()
            .find(|agreement| agreement.id == agreement_id)
        {
            if let Some(existing) = agreement
                .lines
                .iter_mut()
                .find(|existing| existing.id == line.id)
            {
                *existing = line;
            } else {
                agreement.lines.push(line);
            }
            agreement.total_amount = agreement.lines.iter().map(|line| line.amount).sum();
        }
        self.funding_pending = false;
        self.funding_error = None;
        self.new_line_description.clear();
        self.new_line_rate.clear();
    }

    pub fn agreement_line_deleted(&mut self, agreement_id: &str, line_id: &str) {
        if let Some(agreement) = self
            .agreements
            .iter_mut()
            .find(|agreement| agreement.id == agreement_id)
        {
            agreement.lines.retain(|line| line.id != line_id);
            agreement.total_amount = agreement.lines.iter().map(|line| line.amount).sum();
        }
        self.funding_pending = false;
        self.funding_error = None;
    }

    pub fn agreement_finalised(&mut self, agreement: ApiAgreement, generated_shifts: usize) {
        if let Some(package) = self
            .funding_packages
            .iter_mut()
            .find(|package| package.id == agreement.funding_package_id)
        {
            package.committed_amount += agreement.total_amount;
            if let Some(budget) = package.budget_amount {
                package.remaining_amount = Some((budget - package.committed_amount).max(0.0));
            }
        }
        if let Some(existing) = self
            .agreements
            .iter_mut()
            .find(|existing| existing.id == agreement.id)
        {
            *existing = agreement;
        }
        self.funding_pending = false;
        self.funding_error = None;
        self.funding_notice = Some(format!(
            "Agreement finalised — {generated_shifts} draft shift{} generated.",
            if generated_shifts == 1 { "" } else { "s" }
        ));
    }

    pub fn funding_failed(&mut self, message: String) {
        self.funding_pending = false;
        self.funding_error = Some(message);
    }

    pub fn build_write(&self) -> Result<ApiContactWrite, String> {
        if optional(&self.given_name).is_none() && optional(&self.family_name).is_none() {
            return Err("A contact needs a given or family name.".to_owned());
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
            given_name: optional(&self.given_name),
            family_name: optional(&self.family_name),
            preferred_name: optional(&self.preferred_name),
            pronouns: optional(&self.pronouns),
            date_of_birth: optional(&self.date_of_birth),
            gender: optional(&self.gender),
            languages: self
                .languages
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect(),
            cultural_identity: optional(&self.cultural_identity),
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
    roles: &[RoleOption],
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
            if let Some(next) = body(ui, state, catalog, roles) {
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
    roles: &[RoleOption],
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
                    if let Some(next) = body(ui, state, catalog, roles) {
                        action = Some(next);
                    }
                });
        });

    action
}

/// A contact editor rendered inside a column: scrollable body and a fixed
/// footer. The column header owns the record name, pop-out and close actions.
pub fn show_column(
    ui: &mut Ui,
    state: &mut ContactWindowState,
    catalog: &[ApiGraphNode],
    roles: &[RoleOption],
) -> Option<ContactEditorAction> {
    let mut action = None;

    ui.add_space(4.0);
    let footer_height = 54.0;
    let body_height = (ui.available_height() - footer_height).max(120.0);
    ScrollArea::vertical()
        .id_salt(("contact-column-body", &state.contact_id))
        .max_height(body_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if let Some(next) = body(ui, state, catalog, roles) {
                action = Some(next);
            }
        });

    ui.add_space(4.0);
    if let Some(next) = footer(ui, state) {
        action = Some(next);
    }
    action
}

fn header(ui: &mut Ui, state: &mut ContactWindowState) -> Option<ContactEditorAction> {
    let mut action = None;

    ui.horizontal(|ui| {
        let name = state.computed_display_name();
        let title = if name.is_empty() {
            "Contact record".to_owned()
        } else {
            name
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
    roles: &[RoleOption],
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
    if shows_qualities(state) {
        ui.add_space(12.0);
        if let Some(next) = qualities_card(ui, state) {
            action = Some(next);
        }
    }
    if shows_requirements(state) {
        ui.add_space(12.0);
        if let Some(next) = requirements_card(ui, state) {
            action = Some(next);
        }
    }
    if shows_funding(state) {
        ui.add_space(12.0);
        if let Some(next) = funding_card(ui, state) {
            action = Some(next);
        }
    }
    ui.add_space(12.0);
    if let Some(next) = relationships_card(ui, state, catalog, roles) {
        action = Some(next);
    }
    ui.add_space(12.0);
    profile_card(ui, state);
    action
}

fn shows_qualities(state: &ContactWindowState) -> bool {
    matches!(state.role.as_str(), "worker" | "dual") || !state.quality_values.is_empty()
}

fn shows_requirements(state: &ContactWindowState) -> bool {
    matches!(state.role.as_str(), "client" | "dual") || !state.requirements.is_empty()
}

fn shows_funding(state: &ContactWindowState) -> bool {
    matches!(state.role.as_str(), "client" | "dual")
        || !state.funding_packages.is_empty()
        || !state.agreements.is_empty()
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

        let display_name = state.computed_display_name();
        ui.label(
            RichText::new("Display name")
                .color(theme::muted())
                .size(10.5),
        );
        ui.label(
            RichText::new(if display_name.is_empty() {
                "Enter a given or family name".to_owned()
            } else {
                display_name
            })
            .font(theme::strong_font(13.0)),
        );
        ui.label(
            RichText::new("Calculated automatically from the name fields below.")
                .color(theme::faint())
                .size(10.0),
        );
        ui.add_space(8.0);

        field_row(ui, |ui, right| {
            if right {
                text_field(ui, "Family name", &mut state.family_name);
            } else {
                text_field(ui, "Given name", &mut state.given_name);
            }
        });
        field_row(ui, |ui, right| {
            if right {
                text_field(ui, "Pronouns", &mut state.pronouns);
            } else {
                text_field(ui, "Preferred name", &mut state.preferred_name);
            }
        });
        field_row(ui, |ui, right| {
            if right {
                text_field(ui, "Email", &mut state.email);
            } else {
                text_field(ui, "Date of birth (YYYY-MM-DD)", &mut state.date_of_birth);
            }
        });
        field_row(ui, |ui, right| {
            if right {
                text_field(
                    ui,
                    "Languages spoken (comma separated)",
                    &mut state.languages,
                );
            } else {
                field_label(ui, "Gender", false);
                gender_select(ui, state);
            }
        });
        field_row(ui, |ui, right| {
            if right {
                text_field(ui, "Phone", &mut state.phone);
            } else {
                text_field(ui, "Cultural identity", &mut state.cultural_identity);
            }
        });
        field_row(ui, |ui, right| {
            if right {
                field_label(ui, "Status", false);
                status_select(ui, state);
            } else {
                field_label(ui, "Role", false);
                role_summary(ui, &state.role);
            }
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

fn qualities_card(ui: &mut Ui, state: &mut ContactWindowState) -> Option<ContactEditorAction> {
    let mut action = None;

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Skills & training").font(theme::strong_font(15.0)));
        ui.label(
            RichText::new(
                "Record a skill only when the worker holds it. Absence means not recorded; there is no need to mark every other skill as not held.",
            )
            .color(theme::muted())
            .size(10.5),
        );
        ui.add_space(8.0);

        if let Some(error) = &state.quality_error {
            error_box(ui, error);
            ui.add_space(6.0);
        }

        if state.quality_values.is_empty() {
            ui.label(
                RichText::new("No skills or training recorded.")
                    .color(theme::faint())
                    .size(11.0),
            );
        }

        let values = state.quality_values.clone();
        for value in &values {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&value.quality_name).strong());
                let colour = if value.is_expired() {
                    theme::danger()
                } else if value.is_expiring() {
                    theme::warning()
                } else {
                    theme::muted()
                };
                ui.label(RichText::new(value.summary()).color(colour).size(10.5));
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
                        action =
                            Some(ContactEditorAction::DeleteQualityValue(value.quality_id.clone()));
                    }
                });
            });
        }

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);

        let catalogue = state
            .detail
            .as_ref()
            .map(|detail| {
                detail
                    .quality_catalogue
                    .iter()
                    .filter(|quality| !quality.is_intrinsic() && quality.is_active)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if catalogue.is_empty() {
            ui.label(
                RichText::new(
                    "No skills are configured yet. An administrator can add them to the catalogue.",
                )
                .color(theme::faint())
                .size(11.0),
            );
            return;
        }

        field_label(ui, "Add a skill or training", false);
        let selected = state
            .new_quality_id
            .as_ref()
            .and_then(|id| catalogue.iter().find(|quality| &quality.id == id).cloned());
        let selected_text = selected.as_ref().map_or_else(
            || "Choose a skill…".to_owned(),
            |quality| format!("{} — {}", quality.name, quality.category_label()),
        );
        let combo = egui::ComboBox::from_id_salt("new-quality")
            .selected_text(selected_text)
            .width(ui.available_width().min(380.0))
            .show_ui(ui, |ui| {
                for quality in &catalogue {
                    let checked = state.new_quality_id.as_deref() == Some(quality.id.as_str());
                    if ui
                        .selectable_label(
                            checked,
                            format!("{} — {}", quality.name, quality.category_label()),
                        )
                        .clicked()
                    {
                        state.new_quality_id = Some(quality.id.clone());
                        state.new_quality_held = true;
                        state.new_quality_value.clear();
                        state.new_quality_expires.clear();
                        state.new_quality_evidence.clear();
                    }
                }
            });
        named(&combo.response, "Skill or training");

        let Some(quality) = selected else {
            return;
        };

        ui.add_space(4.0);
        quality_value_input(
            ui,
            "new-quality-value",
            &quality,
            &mut state.new_quality_held,
            &mut state.new_quality_value,
        );
        if quality.tracks_expiry {
            date_field(
                ui,
                "quality-expiry",
                "Expiry",
                &mut state.new_quality_expires,
            );
        }
        text_field(
            ui,
            "Evidence or certificate reference",
            &mut state.new_quality_evidence,
        );

        ui.add_space(4.0);
        if ui
            .add_enabled(
                !state.quality_pending,
                theme::secondary_button("Add skill"),
            )
            .clicked()
        {
            match build_quality_value(state, &quality) {
                Ok(payload) => {
                    state.quality_pending = true;
                    state.quality_error = None;
                    action = Some(ContactEditorAction::SetQualityValue(payload));
                }
                Err(message) => state.quality_error = Some(message),
            }
        }
        if state.quality_pending {
            ui.label(RichText::new("Saving…").color(theme::muted()).size(10.5));
        }
    });

    action
}

fn requirements_card(ui: &mut Ui, state: &mut ContactWindowState) -> Option<ContactEditorAction> {
    let mut action = None;

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Support requirements").font(theme::strong_font(15.0)));
        ui.label(
            RichText::new(
                "What this client needs from the workers who support them. Unmet requirements warn and rank a worker lower; they do not exclude them unless you choose Block.",
            )
            .color(theme::muted())
            .size(10.5),
        );
        ui.add_space(8.0);

        if let Some(error) = &state.requirement_error {
            error_box(ui, error);
            ui.add_space(6.0);
        }

        if state.requirements.is_empty() {
            ui.label(
                RichText::new("No requirements recorded.")
                    .color(theme::faint())
                    .size(11.0),
            );
        }

        let requirements = state.requirements.clone();
        for requirement in &requirements {
            ui.horizontal(|ui| {
                ui.label(RichText::new(requirement.summary()).strong());
                if requirement.is_block() {
                    theme::pill(ui, "BLOCK", theme::danger());
                } else {
                    theme::pill(ui, "WARN", theme::warning());
                }
                if let Some(service) = &requirement.service_name {
                    ui.label(RichText::new(service).color(theme::faint()).size(10.0));
                }
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
                        action =
                            Some(ContactEditorAction::DeleteRequirement(requirement.id.clone()));
                    }
                });
            });
            if let Some(reason) = &requirement.reason {
                ui.label(RichText::new(reason).color(theme::faint()).size(10.0));
            }
        }

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);

        let catalogue = state
            .detail
            .as_ref()
            .map(|detail| {
                detail
                    .quality_catalogue
                    .iter()
                    .filter(|quality| quality.is_active)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if catalogue.is_empty() {
            ui.label(
                RichText::new("No qualities are configured yet.")
                    .color(theme::faint())
                    .size(11.0),
            );
            return;
        }

        field_label(ui, "Add a requirement", false);
        let selected = state
            .new_requirement_quality_id
            .as_ref()
            .and_then(|id| catalogue.iter().find(|quality| &quality.id == id).cloned());
        let selected_text = selected.as_ref().map_or_else(
            || "Choose a quality…".to_owned(),
            |quality| format!("{} — {}", quality.name, quality.category_label()),
        );
        let combo = egui::ComboBox::from_id_salt("new-requirement-quality")
            .selected_text(selected_text)
            .width(ui.available_width().min(380.0))
            .show_ui(ui, |ui| {
                for quality in &catalogue {
                    let checked =
                        state.new_requirement_quality_id.as_deref() == Some(quality.id.as_str());
                    if ui
                        .selectable_label(
                            checked,
                            format!("{} — {}", quality.name, quality.category_label()),
                        )
                        .clicked()
                    {
                        state.new_requirement_quality_id = Some(quality.id.clone());
                        state.new_requirement_operator = "exists".to_owned();
                        state.new_requirement_value.clear();
                        state.new_requirement_held = true;
                    }
                }
            });
        named(&combo.response, "Requirement quality");

        let Some(quality) = selected else {
            return;
        };

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            field_label(ui, "Rule", false);
            let combo = egui::ComboBox::from_id_salt("new-requirement-operator")
                .selected_text(operator_label(&state.new_requirement_operator))
                .show_ui(ui, |ui| {
                    for operator in ["exists", "equals", "not_equals", "in", "contains"] {
                        ui.selectable_value(
                            &mut state.new_requirement_operator,
                            operator.to_owned(),
                            operator_label(operator),
                        );
                    }
                });
            named(&combo.response, "Requirement rule");
        });

        if state.new_requirement_operator != "exists" {
            if state.new_requirement_operator == "in" {
                text_field(
                    ui,
                    "Allowed values (comma separated)",
                    &mut state.new_requirement_value,
                );
            } else if state.new_requirement_operator == "contains" {
                text_field(ui, "Value to include", &mut state.new_requirement_value);
            } else {
                quality_value_input(
                    ui,
                    "new-requirement-value",
                    &quality,
                    &mut state.new_requirement_held,
                    &mut state.new_requirement_value,
                );
            }
        }

        ui.horizontal(|ui| {
            field_label(ui, "If unmet", false);
            let combo = egui::ComboBox::from_id_salt("new-requirement-severity")
                .selected_text(if state.new_requirement_severity == "block" {
                    "Block"
                } else {
                    "Warn and rank lower"
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut state.new_requirement_severity,
                        "warn".to_owned(),
                        "Warn and rank lower",
                    );
                    ui.selectable_value(
                        &mut state.new_requirement_severity,
                        "block".to_owned(),
                        "Block",
                    );
                });
            named(&combo.response, "Unmet requirement severity");
        });

        text_field(
            ui,
            if quality.is_sensitive {
                "Reason or legal basis (required)"
            } else {
                "Reason (optional)"
            },
            &mut state.new_requirement_reason,
        );

        ui.add_space(4.0);
        if ui
            .add_enabled(
                !state.requirement_pending,
                theme::secondary_button("Add requirement"),
            )
            .clicked()
        {
            match build_requirement(state, &quality) {
                Ok(payload) => {
                    state.requirement_pending = true;
                    state.requirement_error = None;
                    action = Some(ContactEditorAction::SetRequirement(payload));
                }
                Err(message) => state.requirement_error = Some(message),
            }
        }
        if state.requirement_pending {
            ui.label(RichText::new("Saving…").color(theme::muted()).size(10.5));
        }
    });

    action
}

fn quality_value_input(
    ui: &mut Ui,
    id: &str,
    quality: &ApiWorkerQuality,
    held: &mut bool,
    text: &mut String,
) {
    match quality.data_type.as_str() {
        "boolean" => {
            ui.checkbox(held, "Held");
        }
        "select" => {
            let options = quality.options.clone().unwrap_or_default();
            if text.is_empty() {
                *text = options.first().cloned().unwrap_or_default();
            }
            let combo = egui::ComboBox::from_id_salt(format!("{id}-{}", quality.id))
                .selected_text(if text.is_empty() {
                    "Choose…".to_owned()
                } else {
                    quality_value_label(&Value::String(text.clone()))
                })
                .show_ui(ui, |ui| {
                    for option in &options {
                        ui.selectable_value(
                            text,
                            option.clone(),
                            quality_value_label(&Value::String(option.clone())),
                        );
                    }
                });
            named(&combo.response, "Quality value");
        }
        "multiselect" => {
            text_field(ui, "Values (comma separated)", text);
        }
        "date" => {
            text_field(ui, "Date (YYYY-MM-DD)", text);
        }
        _ => {
            text_field(ui, "Value", text);
        }
    }
}

fn build_quality_value(
    state: &ContactWindowState,
    quality: &ApiWorkerQuality,
) -> Result<ApiWorkerQualityValueWrite, String> {
    let value = quality_input_value(quality, state.new_quality_held, &state.new_quality_value)?;
    let expires_on = optional(&state.new_quality_expires);
    if let Some(expires_on) = &expires_on
        && chrono::NaiveDate::parse_from_str(expires_on, "%Y-%m-%d").is_err()
    {
        return Err("Expiry must be a valid date (YYYY-MM-DD).".to_owned());
    }
    Ok(ApiWorkerQualityValueWrite {
        quality_id: quality.id.clone(),
        value,
        expires_on,
        evidence: optional(&state.new_quality_evidence),
        notes: None,
    })
}

fn build_requirement(
    state: &ContactWindowState,
    quality: &ApiWorkerQuality,
) -> Result<ApiClientRequirementWrite, String> {
    let operator = state.new_requirement_operator.clone();
    let value = if operator == "exists" {
        None
    } else if operator == "in" {
        let values = state
            .new_requirement_value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| Value::String(value.to_owned()))
            .collect::<Vec<_>>();
        if values.is_empty() {
            return Err("Enter at least one allowed value.".to_owned());
        }
        Some(Value::Array(values))
    } else if operator == "contains" {
        let text = state.new_requirement_value.trim();
        if text.is_empty() {
            return Err("Enter the value the worker must include.".to_owned());
        }
        Some(Value::String(text.to_owned()))
    } else {
        Some(quality_input_value(
            quality,
            state.new_requirement_held,
            &state.new_requirement_value,
        )?)
    };

    let reason = optional(&state.new_requirement_reason);
    if quality.is_sensitive && reason.is_none() {
        return Err(format!(
            "{} is sensitive; record a reason or legal basis.",
            quality.name
        ));
    }

    Ok(ApiClientRequirementWrite {
        quality_id: quality.id.clone(),
        operator,
        value,
        severity: Some(state.new_requirement_severity.clone()),
        reason,
        legal_basis: None,
        service_id: None,
        start_date: None,
        end_date: None,
    })
}

fn quality_input_value(
    quality: &ApiWorkerQuality,
    held: bool,
    text: &str,
) -> Result<Value, String> {
    match quality.data_type.as_str() {
        "boolean" => Ok(Value::Bool(held)),
        "select" => {
            if text.trim().is_empty() {
                return Err(format!("Choose a value for {}.", quality.name));
            }
            Ok(Value::String(text.trim().to_owned()))
        }
        "multiselect" => {
            let values = text
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| Value::String(value.to_owned()))
                .collect::<Vec<_>>();
            if values.is_empty() {
                return Err(format!("Enter at least one value for {}.", quality.name));
            }
            Ok(Value::Array(values))
        }
        "date" => {
            let text = text.trim();
            if chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").is_err() {
                return Err("Date must be YYYY-MM-DD.".to_owned());
            }
            Ok(Value::String(text.to_owned()))
        }
        _ => {
            if text.trim().is_empty() {
                return Err(format!("Enter a value for {}.", quality.name));
            }
            Ok(Value::String(text.trim().to_owned()))
        }
    }
}

fn operator_label(operator: &str) -> &str {
    match operator {
        "equals" => "must be",
        "not_equals" => "must not be",
        "in" => "must be one of",
        "contains" => "must include",
        _ => "must be held",
    }
}

fn funding_card(ui: &mut Ui, state: &mut ContactWindowState) -> Option<ContactEditorAction> {
    let mut action = None;

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Funding & agreements").font(theme::strong_font(15.0)));
        ui.label(
            RichText::new(
                "Funding packages, quotes and service agreements. Finalising an agreement generates draft shifts you can fill with workers.",
            )
            .color(theme::muted())
            .size(10.5),
        );
        ui.add_space(8.0);

        if let Some(error) = &state.funding_error {
            error_box(ui, error);
            ui.add_space(6.0);
        }
        if let Some(notice) = &state.funding_notice {
            ui.label(RichText::new(notice).color(theme::success()).size(10.5));
            ui.add_space(4.0);
        }

        ui.label(RichText::new("Funding packages").font(theme::strong_font(12.0)));
        if state.funding_packages.is_empty() {
            ui.label(
                RichText::new("No funding packages yet.")
                    .color(theme::faint())
                    .size(11.0),
            );
        }
        let packages = state.funding_packages.clone();
        for package in &packages {
            Frame::new()
                .fill(theme::surface_raised())
                .corner_radius(CornerRadius::same(7))
                .inner_margin(Margin::same(7))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&package.name).strong().size(11.0));
                        theme::pill(
                            ui,
                            funding_kind_label(&package.source_kind),
                            theme::accent_blue(),
                        );
                        ui.label(
                            RichText::new(&package.source_name)
                                .color(theme::muted())
                                .size(10.0),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(&package.status)
                                    .color(theme::faint())
                                    .size(9.5),
                            );
                        });
                    });
                    ui.label(
                        RichText::new(format!(
                            "{} → {}",
                            package.start_date,
                            package.end_date.as_deref().unwrap_or("open")
                        ))
                        .color(theme::muted())
                        .size(9.5),
                    );
                    if let Some(reference) = &package.reference {
                        ui.label(
                            RichText::new(reference)
                                .color(theme::faint())
                                .size(9.5),
                        );
                    }
                    if let Some(budget) = package.budget_amount {
                        ui.label(
                            RichText::new(format!(
                                "Budget ${budget:.2} · committed ${:.2} · remaining ${:.2}",
                                package.committed_amount,
                                package.remaining_amount.unwrap_or(0.0)
                            ))
                            .color(theme::muted())
                            .size(9.5),
                        );
                    }
                });
            ui.add_space(4.0);
        }

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);

        let sources = state
            .funding_sources
            .iter()
            .filter(|source| source.is_active)
            .cloned()
            .collect::<Vec<_>>();
        if sources.is_empty() {
            ui.label(
                RichText::new(
                    "No funding sources are configured. An administrator can add them under Funding sources.",
                )
                .color(theme::faint())
                .size(11.0),
            );
        } else {
            field_label(ui, "Add a funding package", false);
            let selected = state
                .new_package_source_id
                .as_ref()
                .and_then(|id| sources.iter().find(|source| &source.id == id).cloned());
            let selected_text = selected.as_ref().map_or_else(
                || "Choose a funding source…".to_owned(),
                |source| {
                    format!(
                        "{} · {}",
                        source.name,
                        claiming_model_label(&source.claiming_model)
                    )
                },
            );
            let combo = egui::ComboBox::from_id_salt("new-package-source")
                .selected_text(selected_text)
                .width(ui.available_width().min(380.0))
                .show_ui(ui, |ui| {
                    for source in &sources {
                        if ui
                            .selectable_label(
                                state.new_package_source_id.as_deref() == Some(source.id.as_str()),
                                format!("{} · {}", source.name, funding_kind_label(&source.kind)),
                            )
                            .clicked()
                        {
                            state.new_package_source_id = Some(source.id.clone());
                        }
                    }
                });
            named(&combo.response, "Funding source");
            text_field(ui, "Package name", &mut state.new_package_name);
            text_field(ui, "Plan or package reference", &mut state.new_package_reference);
            field_row(ui, |ui, right| {
                if right {
                    date_field(
                        ui,
                        "package-end",
                        "End (optional)",
                        &mut state.new_package_end,
                    );
                } else {
                    date_field(ui, "package-start", "Start", &mut state.new_package_start);
                }
            });
            text_field(ui, "Budget (optional)", &mut state.new_package_budget);
            if ui
                .add_enabled(
                    !state.funding_pending,
                    theme::secondary_button("Add package"),
                )
                .clicked()
            {
                match build_funding_package(state) {
                    Ok(payload) => {
                        state.funding_pending = true;
                        state.funding_error = None;
                        state.funding_notice = None;
                        action = Some(ContactEditorAction::AddFundingPackage(payload));
                    }
                    Err(message) => state.funding_error = Some(message),
                }
            }
        }

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(8.0);
        ui.label(RichText::new("Quotes & service agreements").font(theme::strong_font(12.0)));

        let agreements = state.agreements.clone();
        for agreement in &agreements {
            agreement_block(ui, state, agreement, &mut action);
            ui.add_space(4.0);
        }

        if packages.is_empty() {
            ui.label(
                RichText::new("Add a funding package before creating a quote.")
                    .color(theme::faint())
                    .size(11.0),
            );
        } else {
            field_label(ui, "Add a quote", false);
            let selected = state
                .new_agreement_package_id
                .as_ref()
                .and_then(|id| packages.iter().find(|package| &package.id == id).cloned());
            let selected_text = selected.as_ref().map_or_else(
                || "Choose a funding package…".to_owned(),
                |package| package.name.clone(),
            );
            let combo = egui::ComboBox::from_id_salt("new-agreement-package")
                .selected_text(selected_text)
                .width(ui.available_width().min(380.0))
                .show_ui(ui, |ui| {
                    for package in &packages {
                        if ui
                            .selectable_label(
                                state.new_agreement_package_id.as_deref()
                                    == Some(package.id.as_str()),
                                &package.name,
                            )
                            .clicked()
                        {
                            state.new_agreement_package_id = Some(package.id.clone());
                        }
                    }
                });
            named(&combo.response, "Funding package");
            text_field(ui, "Quote title", &mut state.new_agreement_title);
            field_row(ui, |ui, right| {
                if right {
                    date_field(
                        ui,
                        "agreement-end",
                        "End (required for recurring lines)",
                        &mut state.new_agreement_end,
                    );
                } else {
                    date_field(
                        ui,
                        "agreement-start",
                        "Start",
                        &mut state.new_agreement_start,
                    );
                }
            });
            if ui
                .add_enabled(
                    !state.funding_pending,
                    theme::secondary_button("Add quote"),
                )
                .clicked()
            {
                match build_agreement(state) {
                    Ok(payload) => {
                        state.funding_pending = true;
                        state.funding_error = None;
                        state.funding_notice = None;
                        action = Some(ContactEditorAction::AddAgreement(payload));
                    }
                    Err(message) => state.funding_error = Some(message),
                }
            }
        }
    });

    action
}

fn agreement_block(
    ui: &mut Ui,
    state: &mut ContactWindowState,
    agreement: &ApiAgreement,
    action: &mut Option<ContactEditorAction>,
) {
    Frame::new()
        .fill(theme::surface_raised())
        .corner_radius(CornerRadius::same(7))
        .inner_margin(Margin::same(7))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(&agreement.title).strong().size(11.0));
                let colour = match agreement.status.as_str() {
                    "active" => theme::success(),
                    "quoted" | "accepted" => theme::warning(),
                    _ => theme::faint(),
                };
                theme::pill(ui, agreement_status_label(&agreement.status), colour);
                ui.label(
                    RichText::new(format!("${:.2}", agreement.total_amount))
                        .color(theme::muted())
                        .size(10.0),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if agreement.finalised_at.is_none()
                        && matches!(agreement.status.as_str(), "quoted" | "accepted")
                        && ui
                            .add(theme::secondary_button("Finalise & generate rosters"))
                            .clicked()
                    {
                        *action =
                            Some(ContactEditorAction::FinaliseAgreement(agreement.id.clone()));
                    }
                });
            });
            ui.label(
                RichText::new(format!(
                    "{} → {}",
                    agreement.start_date,
                    agreement.end_date.as_deref().unwrap_or("open")
                ))
                .color(theme::muted())
                .size(9.5),
            );

            for line in &agreement.lines {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&line.description).size(10.5));
                    ui.label(
                        RichText::new(format!(
                            "{} {} × ${:.2} = ${:.2}",
                            line.quantity, line.unit, line.unit_rate, line.amount
                        ))
                        .color(theme::muted())
                        .size(9.5),
                    );
                    if let Some(service) = &line.service_name {
                        ui.label(RichText::new(service).color(theme::faint()).size(9.5));
                    }
                    if line.recurrence != "none" {
                        ui.label(
                            RichText::new(format!(
                                "{} {}",
                                line.recurrence,
                                weekdays_label(&line.weekdays)
                            ))
                            .color(theme::faint())
                            .size(9.5),
                        );
                    }
                    if agreement.finalised_at.is_none() {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Remove").color(theme::danger()).size(9.5),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                *action = Some(ContactEditorAction::DeleteAgreementLine {
                                    agreement_id: agreement.id.clone(),
                                    line_id: line.id.clone(),
                                });
                            }
                        });
                    }
                });
            }

            if agreement.finalised_at.is_some() {
                return;
            }

            if state.new_line_agreement_id.as_deref() == Some(agreement.id.as_str()) {
                ui.add_space(4.0);
                agreement_line_form(ui, state, agreement, action);
            } else if ui
                .add(
                    egui::Button::new(
                        RichText::new("+ Add line")
                            .color(theme::accent())
                            .size(10.0),
                    )
                    .frame(false),
                )
                .clicked()
            {
                state.new_line_agreement_id = Some(agreement.id.clone());
                state.new_line_description.clear();
                state.new_line_rate.clear();
            }
        });
}

fn agreement_line_form(
    ui: &mut Ui,
    state: &mut ContactWindowState,
    agreement: &ApiAgreement,
    action: &mut Option<ContactEditorAction>,
) {
    let services = state.funding_services.clone();
    let selected_text = state
        .new_line_service_id
        .as_ref()
        .and_then(|id| {
            services
                .iter()
                .find(|service| &service.id == id)
                .map(|service| service.name.clone())
        })
        .unwrap_or_else(|| "No service".to_owned());
    let combo = egui::ComboBox::from_id_salt(("line-service", &agreement.id))
        .selected_text(selected_text)
        .width(ui.available_width().min(360.0))
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(state.new_line_service_id.is_none(), "No service")
                .clicked()
            {
                state.new_line_service_id = None;
            }
            for service in &services {
                if ui
                    .selectable_label(
                        state.new_line_service_id.as_deref() == Some(service.id.as_str()),
                        &service.name,
                    )
                    .clicked()
                {
                    state.new_line_service_id = Some(service.id.clone());
                }
            }
        });
    named(&combo.response, "Service");
    text_field(ui, "Line description", &mut state.new_line_description);
    field_row(ui, |ui, right| {
        if right {
            text_field(ui, "Unit rate", &mut state.new_line_rate);
        } else {
            text_field(ui, "Quantity", &mut state.new_line_quantity);
        }
    });
    ui.horizontal(|ui| {
        field_label(ui, "Unit", false);
        let combo = egui::ComboBox::from_id_salt(("line-unit", &agreement.id))
            .selected_text(&state.new_line_unit)
            .show_ui(ui, |ui| {
                for unit in ["hour", "session", "each", "day"] {
                    ui.selectable_value(&mut state.new_line_unit, unit.to_owned(), unit);
                }
            });
        named(&combo.response, "Unit");
    });
    ui.horizontal(|ui| {
        field_label(ui, "Repeats", false);
        let combo = egui::ComboBox::from_id_salt(("line-recurrence", &agreement.id))
            .selected_text(&state.new_line_recurrence)
            .show_ui(ui, |ui| {
                for recurrence in ["none", "daily", "weekly", "fortnightly"] {
                    ui.selectable_value(
                        &mut state.new_line_recurrence,
                        recurrence.to_owned(),
                        recurrence,
                    );
                }
            });
        named(&combo.response, "Repeats");
    });
    if state.new_line_recurrence != "none" {
        ui.horizontal_wrapped(|ui| {
            field_label(ui, "Days", false);
            for (value, label) in [
                (1, "Mon"),
                (2, "Tue"),
                (3, "Wed"),
                (4, "Thu"),
                (5, "Fri"),
                (6, "Sat"),
                (7, "Sun"),
            ] {
                let mut checked = state.new_line_weekdays.contains(&value);
                if ui.checkbox(&mut checked, label).changed() {
                    if checked {
                        state.new_line_weekdays.push(value);
                    } else {
                        state.new_line_weekdays.retain(|day| *day != value);
                    }
                    state.new_line_weekdays.sort_unstable();
                }
            }
        });
        field_row(ui, |ui, right| {
            if right {
                text_field(ui, "Duration (minutes)", &mut state.new_line_duration);
            } else {
                text_field(ui, "Start time (HH:MM)", &mut state.new_line_start_time);
            }
        });
    }

    ui.horizontal(|ui| {
        if ui
            .add_enabled(!state.funding_pending, theme::secondary_button("Add line"))
            .clicked()
        {
            match build_agreement_line(state, &agreement.id) {
                Ok(payload) => {
                    state.funding_pending = true;
                    state.funding_error = None;
                    *action = Some(ContactEditorAction::AddAgreementLine {
                        agreement_id: agreement.id.clone(),
                        payload,
                    });
                }
                Err(message) => state.funding_error = Some(message),
            }
        }
        if ui
            .add(
                egui::Button::new(RichText::new("Cancel").color(theme::faint()).size(10.0))
                    .frame(false),
            )
            .clicked()
        {
            state.new_line_agreement_id = None;
        }
    });
}

fn build_funding_package(state: &ContactWindowState) -> Result<ApiFundingPackageWrite, String> {
    let funding_source_id = state
        .new_package_source_id
        .clone()
        .ok_or_else(|| "Choose a funding source.".to_owned())?;
    let name = state.new_package_name.trim();
    if name.is_empty() {
        return Err("Give the package a name.".to_owned());
    }
    let start_date = parse_date(&state.new_package_start, "Package start")?;
    let end_date = optional_date(&state.new_package_end, "Package end")?;
    let budget_amount = if state.new_package_budget.trim().is_empty() {
        None
    } else {
        Some(
            state
                .new_package_budget
                .trim()
                .parse::<f64>()
                .map_err(|_| "Budget must be a number.".to_owned())?,
        )
    };
    Ok(ApiFundingPackageWrite {
        funding_source_id,
        name: name.to_owned(),
        reference: optional(&state.new_package_reference),
        start_date,
        end_date,
        budget_amount,
        notes: None,
    })
}

fn build_agreement(state: &ContactWindowState) -> Result<ApiAgreementWrite, String> {
    let funding_package_id = state
        .new_agreement_package_id
        .clone()
        .ok_or_else(|| "Choose a funding package.".to_owned())?;
    let title = state.new_agreement_title.trim();
    if title.is_empty() {
        return Err("Give the quote a title.".to_owned());
    }
    let start_date = parse_date(&state.new_agreement_start, "Agreement start")?;
    let end_date = optional_date(&state.new_agreement_end, "Agreement end")?;
    Ok(ApiAgreementWrite {
        funding_package_id,
        title: title.to_owned(),
        status: Some("quoted".to_owned()),
        quote_date: Some(start_date.clone()),
        start_date,
        end_date,
        notes: None,
    })
}

fn build_agreement_line(
    state: &ContactWindowState,
    _agreement_id: &str,
) -> Result<ApiAgreementLineWrite, String> {
    let description = state.new_line_description.trim();
    if description.is_empty() {
        return Err("Describe the service on this line.".to_owned());
    }
    let quantity = state
        .new_line_quantity
        .trim()
        .parse::<f64>()
        .map_err(|_| "Quantity must be a number.".to_owned())?;
    let unit_rate = state
        .new_line_rate
        .trim()
        .parse::<f64>()
        .map_err(|_| "Unit rate must be a number.".to_owned())?;
    let recurrence = state.new_line_recurrence.clone();
    let (start_time, duration_minutes) = if recurrence == "none" {
        (None, None)
    } else {
        let start_time =
            chrono::NaiveTime::parse_from_str(state.new_line_start_time.trim(), "%H:%M")
                .or_else(|_| {
                    chrono::NaiveTime::parse_from_str(state.new_line_start_time.trim(), "%H:%M:%S")
                })
                .map_err(|_| "Start time must be HH:MM.".to_owned())?;
        let duration_minutes = state
            .new_line_duration
            .trim()
            .parse::<i32>()
            .map_err(|_| "Duration must be minutes.".to_owned())?;
        (
            Some(start_time.format("%H:%M:%S").to_string()),
            Some(duration_minutes),
        )
    };
    Ok(ApiAgreementLineWrite {
        service_id: state.new_line_service_id.clone(),
        description: description.to_owned(),
        quantity,
        unit: Some(state.new_line_unit.clone()),
        unit_rate,
        recurrence: Some(recurrence),
        weekdays: state.new_line_weekdays.clone(),
        start_time,
        duration_minutes,
        site_id: None,
        notes: None,
    })
}

fn weekdays_label(weekdays: &[i16]) -> String {
    weekdays
        .iter()
        .filter_map(|day| match day {
            1 => Some("Mon"),
            2 => Some("Tue"),
            3 => Some("Wed"),
            4 => Some("Thu"),
            5 => Some("Fri"),
            6 => Some("Sat"),
            7 => Some("Sun"),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn relationships_card(
    ui: &mut Ui,
    state: &mut ContactWindowState,
    catalog: &[ApiGraphNode],
    roles: &[RoleOption],
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

        let edges = state.relationships.clone();
        let (ended, active): (Vec<_>, Vec<_>) = edges.iter().partition(|edge| edge.is_ended());
        for edge in active {
            if let Some(next) = relationship_row(ui, state, edge, catalog, roles) {
                action = Some(next);
            }
        }

        if !ended.is_empty() {
            ui.add_space(2.0);
            let label = format!(
                "{} ended relationship{}",
                ended.len(),
                if ended.len() == 1 { "" } else { "s" }
            );
            let toggle = ui.add(
                egui::Button::new(
                    RichText::new(if state.show_ended_relationships {
                        format!("Hide {label}")
                    } else {
                        format!("Show {label}")
                    })
                    .color(theme::muted())
                    .size(10.5),
                )
                .frame(false),
            );
            if toggle.clicked() {
                state.show_ended_relationships = !state.show_ended_relationships;
            }
            if state.show_ended_relationships {
                ui.add_space(4.0);
                for edge in ended {
                    if let Some(next) = relationship_row(ui, state, edge, catalog, roles) {
                        action = Some(next);
                    }
                }
            }
        }

        // Adding is hidden while a change is being made, so the party lookup
        // serves only one form at a time.
        if state.relationship_form.is_some() {
            return;
        }

        ui.add_space(4.0);
        ui.separator();
        ui.add_space(8.0);
        ui.label(
            RichText::new("Add relationship")
                .font(theme::strong_font(12.5))
                .color(theme::text()),
        );

        role_picker(
            ui,
            "relationship-kind",
            roles,
            &mut state.relationship_role,
            RoleOption::describe,
        );
        let chosen = state.relationship_role.clone();
        if let Some(role) = &chosen {
            ui.label(
                RichText::new(format!(
                    "This record is {} · the linked record is {}",
                    relationship_role_label(&role.subject_role),
                    relationship_role_label(&role.linked_role)
                ))
                .color(theme::faint())
                .size(10.0),
            );
        }

        let exclude = state.contact_id.clone();
        party_lookup(
            ui,
            "relationship-party",
            &mut state.relationship_target,
            Some(&exclude),
            catalog,
        );

        ui.add(
            egui::TextEdit::singleline(&mut state.relationship_notes)
                .hint_text("Relationship notes (optional)")
                .desired_width(ui.available_width())
                .margin(Margin::symmetric(8, 6)),
        );

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let target = state.relationship_target.clone();
            let can_link = target.is_some() && chosen.is_some() && !state.relationship_pending;
            if ui
                .add_enabled(can_link, theme::primary_button("Link relationship"))
                .clicked()
                && let (Some(target), Some(role)) = (target, chosen.clone())
            {
                state.relationship_pending = true;
                state.relationship_error = None;
                action = Some(ContactEditorAction::CreateRelationship {
                    from_role: role.subject_role,
                    to_kind: target.kind,
                    to_id: target.id,
                    to_role: role.linked_role,
                    notes: state.relationship_notes.trim().to_owned(),
                });
            }
            if ui
                .add_enabled(chosen.is_some(), theme::secondary_button("+ New contact"))
                .clicked()
            {
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
            let (from_role, to_role) = chosen
                .as_ref()
                .map(|role| (role.subject_role.clone(), role.linked_role.clone()))
                .unwrap_or_default();
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
                    field_row(ui, |ui, right| {
                        if right {
                            text_field(ui, "Family name", &mut draft.family_name);
                        } else {
                            text_field(ui, "Given name", &mut draft.given_name);
                        }
                    });
                    field_row(ui, |ui, right| {
                        if right {
                            text_field(ui, "Phone", &mut draft.phone);
                        } else {
                            text_field(ui, "Email", &mut draft.email);
                        }
                    });

                    if let Some(error) = &relationship_error {
                        ui.label(RichText::new(error).color(theme::danger()).size(10.5));
                    }

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let valid = !draft.given_name.trim().is_empty()
                            || !draft.family_name.trim().is_empty();
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

/// One relationship: summary, actions menu, and any open form or history.
fn relationship_row(
    ui: &mut Ui,
    state: &mut ContactWindowState,
    edge: &ApiRelationship,
    catalog: &[ApiGraphNode],
    roles: &[RoleOption],
) -> Option<ContactEditorAction> {
    let view = relationship_view(edge, &state.contact_id)?;
    let mut action = None;
    let ended = edge.is_ended();

    Frame::new()
        .fill(if ended {
            theme::surface()
        } else {
            theme::surface_raised()
        })
        .stroke(Stroke::new(1.0, theme::border()))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(relationship_role_label(&view.linked_role))
                        .font(theme::strong_font(12.5))
                        .color(if ended { theme::muted() } else { theme::text() }),
                );
                theme::pill(ui, view.linked_kind.label(), theme::accent_blue());
                if let Some((text, colour)) = status_text(edge) {
                    theme::pill(ui, &text, colour);
                }
                if view.linked_kind == PartyKind::Contact
                    && ui
                        .add(
                            egui::Button::new(
                                RichText::new("Open").color(theme::accent()).size(10.5),
                            )
                            .frame(false),
                        )
                        .clicked()
                {
                    action = Some(ContactEditorAction::OpenContact(view.linked_id.clone()));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.menu_button(RichText::new("Actions…").size(10.5), |ui| {
                        let mut kinds = Vec::new();
                        if !ended {
                            if edge.end_date.is_none() {
                                kinds.push(RelationshipFormKind::End);
                            }
                            kinds.push(RelationshipFormKind::Transfer);
                            kinds.push(RelationshipFormKind::ChangeRole);
                        }
                        kinds.push(RelationshipFormKind::Correct);
                        kinds.push(RelationshipFormKind::Remove);
                        for kind in kinds {
                            if ui.button(kind.title()).clicked() {
                                state.relationship_form = Some(RelationshipForm::new(edge, kind));
                                state.relationship_target = None;
                                ui.close();
                            }
                        }
                    });
                    let showing = state
                        .relationship_history
                        .as_ref()
                        .is_some_and(|history| history.edge_id == edge.id);
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(if showing { "Hide history" } else { "History" })
                                    .color(theme::muted())
                                    .size(10.5),
                            )
                            .frame(false),
                        )
                        .clicked()
                    {
                        if showing {
                            state.relationship_history = None;
                        } else {
                            state.relationship_history = Some(RelationshipHistoryView {
                                edge_id: edge.id.clone(),
                                loading: true,
                                stale: false,
                                items: Vec::new(),
                                error: None,
                            });
                            action = Some(ContactEditorAction::LoadRelationshipHistory(
                                edge.id.clone(),
                            ));
                        }
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
                    "This record is {}{}",
                    relationship_role_label(&view.subject_role),
                    edge.start_date
                        .as_deref()
                        .map(|start| format!(" · since {start}"))
                        .unwrap_or_default()
                ))
                .color(theme::faint())
                .size(10.0),
            );
            if let Some(notes) = edge.notes.as_deref()
                && !notes.trim().is_empty()
            {
                ui.label(RichText::new(notes).color(theme::muted()).size(10.5));
            }

            if state
                .relationship_form
                .as_ref()
                .is_some_and(|form| form.edge_id == edge.id)
                && let Some(next) = relationship_form(ui, state, edge, &view, catalog, roles)
            {
                action = Some(next);
            }

            if let Some(history) = &mut state.relationship_history
                && history.edge_id == edge.id
            {
                if history.stale && !history.loading {
                    history.stale = false;
                    history.loading = true;
                    action = Some(ContactEditorAction::LoadRelationshipHistory(
                        edge.id.clone(),
                    ));
                }
                relationship_history(ui, history);
            }
        });
    ui.add_space(6.0);
    action
}

/// A short status pill: upcoming, ending soon, or ended with its reason.
fn status_text(edge: &ApiRelationship) -> Option<(String, Color32)> {
    let reason = edge
        .end_reason
        .as_deref()
        .map(|reason| format!(" · {}", end_reason_label(reason)))
        .unwrap_or_default();
    match (edge.status.as_str(), edge.end_date.as_deref()) {
        ("ended", Some(end)) => Some((format!("Ended {end}{reason}"), theme::faint())),
        ("upcoming", _) => Some((
            format!("Starts {}", edge.start_date.as_deref().unwrap_or("later")),
            theme::accent_blue(),
        )),
        (_, Some(end)) => Some((format!("Ends {end}{reason}"), theme::warning())),
        _ => None,
    }
}

fn relationship_form(
    ui: &mut Ui,
    state: &mut ContactWindowState,
    edge: &ApiRelationship,
    view: &RelationshipView,
    catalog: &[ApiGraphNode],
    roles: &[RoleOption],
) -> Option<ContactEditorAction> {
    let mut action = None;
    let mut cancel = false;
    let target = state.relationship_target.clone();
    let mut form = state.relationship_form.take()?;
    // Which side of the stored relationship this record is on.
    let this_is_from = edge.from_kind == PartyKind::Contact && edge.from_id == state.contact_id;

    ui.add_space(8.0);
    Frame::new()
        .fill(theme::input())
        .stroke(Stroke::new(1.0, theme::accent().gamma_multiply(0.5)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(form.kind.title())
                    .font(theme::strong_font(12.5))
                    .color(theme::text()),
            );
            ui.label(RichText::new(form.kind.help()).color(theme::faint()).size(10.0));
            ui.add_space(6.0);

            match form.kind {
                RelationshipFormKind::End => {
                    text_field(ui, "Last day (YYYY-MM-DD)", &mut form.date);
                    reason_select(ui, &mut form.reason, "relationship-end-reason");
                }
                RelationshipFormKind::Transfer => {
                    ui.label(
                        RichText::new(format!(
                            "Replace {} with:",
                            view.linked_name
                        ))
                        .color(theme::muted())
                        .size(10.5),
                    );
                    let exclude = state.contact_id.clone();
                    party_lookup(
                        ui,
                        "relationship-transfer-party",
                        &mut state.relationship_target,
                        Some(&exclude),
                        catalog,
                    );
                    text_field(ui, "New party from (YYYY-MM-DD)", &mut form.date);
                    reason_select(ui, &mut form.reason, "relationship-transfer-reason");
                }
                RelationshipFormKind::ChangeRole => {
                    role_picker(
                        ui,
                        "relationship-new-role",
                        roles,
                        &mut form.role,
                        RoleOption::describe,
                    );
                    if let Some(role) = &form.role {
                        ui.label(
                            RichText::new(format!(
                                "This record becomes {} · {} becomes {}",
                                relationship_role_label(&role.subject_role),
                                view.linked_name,
                                relationship_role_label(&role.linked_role)
                            ))
                            .color(theme::faint())
                            .size(10.0),
                        );
                    }
                    text_field(ui, "New roles from (YYYY-MM-DD)", &mut form.date);
                    reason_select(ui, &mut form.reason, "relationship-role-reason");
                }
                RelationshipFormKind::Correct => {
                    if form.role.is_none() {
                        form.role = Some(current_role(edge, this_is_from, roles));
                    }
                    field_label(ui, "Roles", false);
                    role_picker(
                        ui,
                        "relationship-correct-role",
                        roles,
                        &mut form.role,
                        RoleOption::describe,
                    );
                    field_row(ui, |ui, right| {
                        if right {
                            text_field(ui, "End date (YYYY-MM-DD)", &mut form.end_date);
                        } else {
                            text_field(ui, "Start date (YYYY-MM-DD)", &mut form.start_date);
                        }
                    });
                    if !form.end_date.trim().is_empty() {
                        reason_select(ui, &mut form.reason, "relationship-correct-reason");
                    }
                    text_field(ui, "Relationship notes", &mut form.notes);
                }
                RelationshipFormKind::Remove => {}
            }

            let note_label = match form.kind {
                RelationshipFormKind::Correct => "What was wrong? (required)",
                RelationshipFormKind::Remove => "Why was this recorded in error? (required)",
                _ => "Note (optional)",
            };
            field_label(ui, note_label, false);
            ui.add(
                egui::TextEdit::multiline(&mut form.note)
                    .desired_rows(2)
                    .desired_width(ui.available_width())
                    .margin(Margin::symmetric(8, 6)),
            );
            if form.reason == END_REASONS.iter().position(|(key, _)| *key == "incident") {
                ui.label(
                    RichText::new(
                        "Incident notes are only shown to people allowed to read them, managers by default.",
                    )
                    .color(theme::warning())
                    .size(10.0),
                );
            }

            if let Some(error) = &form.error {
                ui.label(RichText::new(error).color(theme::danger()).size(10.5));
            }

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let label = match form.kind {
                    RelationshipFormKind::End => "End relationship",
                    RelationshipFormKind::Transfer => "Transfer",
                    RelationshipFormKind::ChangeRole => "Change role",
                    RelationshipFormKind::Correct => "Save correction",
                    RelationshipFormKind::Remove => "Remove",
                };
                if ui
                    .add_enabled(!form.pending, theme::primary_button(label))
                    .clicked()
                {
                    match build_change(&form, edge, this_is_from, target.as_ref()) {
                        Ok(change) => {
                            form.pending = true;
                            form.error = None;
                            action = Some(ContactEditorAction::ChangeRelationship {
                                id: edge.id.clone(),
                                change,
                            });
                        }
                        Err(error) => form.error = Some(error),
                    }
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    cancel = true;
                }
                if form.pending {
                    ui.label(RichText::new("Saving…").color(theme::warning()).size(10.5));
                }
            });
        });

    if !cancel {
        state.relationship_form = Some(form);
    } else {
        state.relationship_target = None;
    }
    action
}

fn reason_select(ui: &mut Ui, reason: &mut Option<usize>, id: &str) {
    field_label(ui, "Reason", true);
    let mut current = *reason;
    let combo = egui::ComboBox::from_id_salt(id)
        .selected_text(current.map_or("Choose a reason…", |index| END_REASONS[index].1))
        .width(ui.available_width().min(260.0))
        .show_ui(ui, |ui| {
            for (index, (_, label)) in END_REASONS.iter().enumerate() {
                ui.selectable_value(&mut current, Some(index), *label);
            }
        });
    named(&combo.response, "Reason");
    *reason = current;
}

fn parse_date(value: &str, label: &str) -> Result<String, String> {
    chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d")
        .map(|date| date.to_string())
        .map_err(|_| format!("{label} must be a date like 2026-11-30."))
}

fn optional_date(value: &str, label: &str) -> Result<Option<String>, String> {
    if value.trim().is_empty() {
        Ok(None)
    } else {
        parse_date(value, label).map(Some)
    }
}

/// Turn a filled-in form into the change to send, or explain what is missing.
fn build_change(
    form: &RelationshipForm,
    edge: &ApiRelationship,
    this_is_from: bool,
    target: Option<&ApiGraphNode>,
) -> Result<RelationshipChange, String> {
    let reason = || {
        form.reason
            .map(|index| END_REASONS[index].0.to_owned())
            .ok_or_else(|| "Choose a reason.".to_owned())
    };
    let note = form.note.trim().to_owned();
    match form.kind {
        RelationshipFormKind::End => Ok(RelationshipChange::End {
            end_date: parse_date(&form.date, "The last day")?,
            reason: reason()?,
            note,
        }),
        RelationshipFormKind::Transfer => {
            let target = target.ok_or("Choose who takes over.")?;
            Ok(RelationshipChange::Transfer {
                effective_date: parse_date(&form.date, "The start date")?,
                // Replace the other party, never this record.
                replace: if this_is_from { "to" } else { "from" }.to_owned(),
                new_kind: target.kind,
                new_id: target.id.clone(),
                reason: reason()?,
                note,
            })
        }
        RelationshipFormKind::ChangeRole => {
            let role = form.role.as_ref().ok_or("Choose the new relationship.")?;
            let (from_role, to_role) = oriented(role, this_is_from);
            Ok(RelationshipChange::ChangeRole {
                effective_date: parse_date(&form.date, "The start date")?,
                from_role,
                to_role,
                reason: reason()?,
                note,
            })
        }
        RelationshipFormKind::Correct => {
            if note.is_empty() {
                return Err("Explain what was wrong.".to_owned());
            }
            let end_date = optional_date(&form.end_date, "The end date")?;
            let end_reason = match end_date {
                Some(_) => Some(reason()?),
                None => None,
            };
            let (from_role, to_role) = match &form.role {
                Some(role) => oriented(role, this_is_from),
                None => (form.from_role.clone(), form.to_role.clone()),
            };
            Ok(RelationshipChange::Correct {
                relationship: ApiRelationshipCorrection {
                    from_kind: edge.from_kind,
                    from_id: edge.from_id.clone(),
                    from_role,
                    to_kind: edge.to_kind,
                    to_id: edge.to_id.clone(),
                    to_role,
                    start_date: optional_date(&form.start_date, "The start date")?,
                    end_date,
                    end_reason,
                    notes: optional(&form.notes),
                },
                note,
            })
        }
        RelationshipFormKind::Remove => {
            if note.is_empty() {
                return Err("Explain why this was recorded in error.".to_owned());
            }
            Ok(RelationshipChange::Remove { reason: note })
        }
    }
}

/// A chosen option as stored `(from_role, to_role)`, given which side this
/// record is on.
fn oriented(role: &RoleOption, this_is_from: bool) -> (String, String) {
    if this_is_from {
        (role.subject_role.clone(), role.linked_role.clone())
    } else {
        (role.linked_role.clone(), role.subject_role.clone())
    }
}

/// The relationship's current roles as a picker option, keeping its catalogue
/// group when the pair is still offered.
fn current_role(edge: &ApiRelationship, this_is_from: bool, roles: &[RoleOption]) -> RoleOption {
    let (subject, linked) = if this_is_from {
        (&edge.from_role, &edge.to_role)
    } else {
        (&edge.to_role, &edge.from_role)
    };
    roles
        .iter()
        .find(|role| &role.subject_role == subject && &role.linked_role == linked)
        .cloned()
        .unwrap_or_else(|| RoleOption::new(subject, linked, "other"))
}

fn relationship_history(ui: &mut Ui, history: &RelationshipHistoryView) {
    ui.add_space(8.0);
    ui.label(
        RichText::new("History")
            .font(theme::strong_font(11.5))
            .color(theme::text()),
    );
    if history.loading {
        ui.label(
            RichText::new("Loading history…")
                .color(theme::muted())
                .size(10.5),
        );
        return;
    }
    if let Some(error) = &history.error {
        ui.label(RichText::new(error).color(theme::danger()).size(10.5));
        return;
    }
    for event in &history.items {
        let when = event
            .effective_date
            .as_deref()
            .map(|date| format!("effective {date}"))
            .unwrap_or_default();
        let recorded = event.created_at.get(..10).unwrap_or(&event.created_at);
        let mut line = format!("{} · {}", recorded, history_action_label(&event.action));
        if let Some(reason) = event.reason.as_deref() {
            line.push_str(&format!(" · {}", end_reason_label(reason)));
        }
        if !when.is_empty() {
            line.push_str(&format!(" · {when}"));
        }
        if let Some(actor) = event.actor_name.as_deref() {
            line.push_str(&format!(" · by {actor}"));
        }
        ui.label(RichText::new(line).color(theme::muted()).size(10.5));
        if let Some(note) = event.note.as_deref() {
            ui.label(
                RichText::new(format!("   “{note}”"))
                    .color(theme::text())
                    .size(10.5),
            );
        } else if event.note_hidden {
            ui.label(
                RichText::new("   Note restricted to people allowed to read incident notes.")
                    .color(theme::faint())
                    .size(10.0),
            );
        }
    }
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

pub(crate) fn party_lookup(
    ui: &mut Ui,
    id_salt: &str,
    target: &mut Option<ApiGraphNode>,
    exclude_contact: Option<&str>,
    catalog: &[ApiGraphNode],
) {
    let search_id = ui.scope_id().with(("relationship-party-search", id_salt));
    let mut search = ui.data_mut(|data| data.get_temp::<String>(search_id).unwrap_or_default());
    let selected = target
        .as_ref()
        .map(|node| format!("{}  ·  {}", node.label, node.kind.label()))
        .unwrap_or_else(|| "Search contacts and organisations…".to_owned());
    let mut chosen: Option<ApiGraphNode> = None;

    let combo = egui::ComboBox::from_id_salt(("relationship-party", id_salt))
        .selected_text(selected)
        .width(ui.available_width().max(150.0))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
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
                        !(node.kind == PartyKind::Contact
                            && exclude_contact.is_some_and(|id| node.id == id))
                    })
                    .filter(|node| needle.is_empty() || node.label.to_lowercase().contains(&needle))
                {
                    if shown >= 60 {
                        break;
                    }
                    shown += 1;
                    let current = target
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
        *target = Some(node);
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

fn gender_select(ui: &mut Ui, state: &mut ContactWindowState) {
    let mut current = state.gender.clone();
    let response = egui::ComboBox::from_id_salt("contact-gender")
        .selected_text(gender_label(&current))
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for (key, label) in [
                ("", "Not recorded"),
                ("female", "Female"),
                ("male", "Male"),
                ("non_binary", "Non-binary"),
                ("prefer_not_to_say", "Prefer not to say"),
            ] {
                ui.selectable_value(&mut current, key.to_owned(), label);
            }
        });
    named(&response.response, "Gender");
    state.gender = current;
}

fn gender_label(key: &str) -> &str {
    match key {
        "female" => "Female",
        "male" => "Male",
        "non_binary" => "Non-binary",
        "prefer_not_to_say" => "Prefer not to say",
        _ => "Not recorded",
    }
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

/// Lay out two fields side by side when there is room, or stacked when the
/// column is narrow. `add(ui, false)` renders the left field and
/// `add(ui, true)` renders the right field.
fn field_row(ui: &mut Ui, mut add: impl FnMut(&mut Ui, bool)) {
    if ui.available_width() >= 520.0 {
        ui.columns(2, |cols| {
            add(&mut cols[0], false);
            add(&mut cols[1], true);
        });
    } else {
        add(ui, false);
        add(ui, true);
    }
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

/// A validated date field with a calendar popup. The text stays editable, but
/// the value must parse as `YYYY-MM-DD` before it can be saved.
fn date_field(ui: &mut Ui, id: &str, label: &str, value: &mut String) {
    field_label(ui, label, false);
    let field_id = ui.make_persistent_id(id);
    ui.horizontal(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(value)
                .hint_text("YYYY-MM-DD")
                .desired_width(110.0)
                .margin(Margin::symmetric(8, 6)),
        );
        named(&response, label);

        let picker = ui.add(
            egui::Button::new(RichText::new("📅").size(12.0))
                .frame(false)
                .min_size(vec2(24.0, 22.0)),
        );
        named(&picker, "Choose a date");

        let valid = value.trim().is_empty()
            || chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").is_ok();
        if !valid {
            ui.label(
                RichText::new("Use YYYY-MM-DD")
                    .color(theme::danger())
                    .size(10.0),
            );
        }

        egui::Popup::from_toggle_button_response(&picker).show(|ui| {
            if let Some(date) = calendar(ui, value, field_id) {
                *value = date.format("%Y-%m-%d").to_string();
                ui.close();
            }
            if ui.button("Clear").clicked() {
                value.clear();
                ui.close();
            }
        });
    });
    ui.add_space(4.0);
}

/// A compact month calendar. Returns the clicked date, if any.
fn calendar(ui: &mut Ui, value: &str, id: egui::Id) -> Option<chrono::NaiveDate> {
    use chrono::{Datelike, Months, NaiveDate};

    let today = chrono::Local::now().date_naive();
    let selected = NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok();
    let month_key = id.with("month");
    let mut month = ui
        .data(|data| data.get_temp::<NaiveDate>(month_key))
        .unwrap_or_else(|| selected.unwrap_or(today))
        .with_day(1)
        .unwrap_or(today);

    let mut picked = None;
    ui.horizontal(|ui| {
        if ui.small_button("◀").clicked() {
            month = month.checked_sub_months(Months::new(1)).unwrap_or(month);
        }
        ui.label(RichText::new(month.format("%B %Y").to_string()).font(theme::strong_font(11.0)));
        if ui.small_button("▶").clicked() {
            month = month.checked_add_months(Months::new(1)).unwrap_or(month);
        }
    });
    ui.data_mut(|data| data.insert_temp(month_key, month));

    egui::Grid::new(id.with("grid"))
        .num_columns(7)
        .spacing([2.0, 2.0])
        .show(ui, |ui| {
            for day in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] {
                ui.label(RichText::new(day).color(theme::faint()).size(9.0));
            }
            ui.end_row();

            let offset = month.weekday().num_days_from_monday() as i64;
            let next_month = month.checked_add_months(Months::new(1)).unwrap_or(month);
            let days = (next_month - month).num_days();

            let mut column = 0;
            for _ in 0..offset {
                ui.label("");
                column += 1;
            }
            for day in 0..days {
                let date = month + chrono::Duration::days(day);
                let is_selected = selected == Some(date);
                let is_today = date == today;
                let text = if is_today {
                    RichText::new(date.day().to_string())
                        .color(theme::accent())
                        .strong()
                } else {
                    RichText::new(date.day().to_string())
                };
                if ui.selectable_label(is_selected, text).clicked() {
                    picked = Some(date);
                }
                column += 1;
                if column % 7 == 0 {
                    ui.end_row();
                }
            }
        });

    picked
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
    use super::{
        ContactWindowState, RelationshipForm, RelationshipFormKind, build_agreement,
        build_agreement_line, build_change, build_funding_package, relationship_view,
    };
    use crate::contact::{
        ApiGraphNode, ApiRelationship, END_REASONS, PartyKind, RelationshipChange, RoleOption,
    };

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
            end_reason: None,
            status: "current".to_owned(),
            previous_id: None,
            notes: None,
            updated_at: String::new(),
        }
    }

    fn form(kind: RelationshipFormKind) -> RelationshipForm {
        let mut form = RelationshipForm::new(&edge(), kind);
        form.date = "2026-12-01".to_owned();
        form
    }

    fn reason(key: &str) -> Option<usize> {
        END_REASONS.iter().position(|(value, _)| *value == key)
    }

    #[test]
    fn ending_needs_a_reason_and_a_valid_date() {
        let mut end = form(RelationshipFormKind::End);
        assert_eq!(
            build_change(&end, &edge(), true, None),
            Err("Choose a reason.".to_owned())
        );
        end.reason = reason("moved_away");
        end.date = "1/12/2026".to_owned();
        assert!(build_change(&end, &edge(), true, None).is_err());
        end.date = "2026-12-01".to_owned();
        assert_eq!(
            build_change(&end, &edge(), true, None),
            Ok(RelationshipChange::End {
                end_date: "2026-12-01".to_owned(),
                reason: "moved_away".to_owned(),
                note: String::new(),
            })
        );
    }

    #[test]
    fn transfer_replaces_the_other_party_not_this_record() {
        let mut transfer = form(RelationshipFormKind::Transfer);
        transfer.reason = reason("changed_provider");
        let target = ApiGraphNode {
            kind: PartyKind::Organisation,
            id: "new-clinic".to_owned(),
            label: "New clinic".to_owned(),
            degree: 0,
            depth: None,
        };
        let Ok(RelationshipChange::Transfer {
            replace, new_id, ..
        }) = build_change(&transfer, &edge(), true, Some(&target))
        else {
            panic!("expected a transfer");
        };
        assert_eq!((replace.as_str(), new_id.as_str()), ("to", "new-clinic"));

        let Ok(RelationshipChange::Transfer { replace, .. }) =
            build_change(&transfer, &edge(), false, Some(&target))
        else {
            panic!("expected a transfer");
        };
        assert_eq!(replace, "from");
        assert_eq!(
            build_change(&transfer, &edge(), true, None),
            Err("Choose who takes over.".to_owned())
        );
    }

    #[test]
    fn change_role_keeps_each_role_on_its_own_side() {
        let mut change = form(RelationshipFormKind::ChangeRole);
        change.reason = reason("circumstances_changed");
        change.role = Some(RoleOption::new("employee", "employer", "work"));
        // This record is the `to` side, so its new role goes in `to_role`.
        let Ok(RelationshipChange::ChangeRole {
            from_role, to_role, ..
        }) = build_change(&change, &edge(), false, None)
        else {
            panic!("expected a role change");
        };
        assert_eq!(
            (from_role.as_str(), to_role.as_str()),
            ("employer", "employee")
        );
    }

    #[test]
    fn corrections_and_removals_must_explain_themselves() {
        let mut correct = form(RelationshipFormKind::Correct);
        assert_eq!(
            build_change(&correct, &edge(), true, None),
            Err("Explain what was wrong.".to_owned())
        );
        correct.note = "Wrong start date".to_owned();
        correct.end_date = "2026-12-31".to_owned();
        assert_eq!(
            build_change(&correct, &edge(), true, None),
            Err("Choose a reason.".to_owned()),
            "an end date always needs a reason"
        );

        let mut remove = form(RelationshipFormKind::Remove);
        assert!(build_change(&remove, &edge(), true, None).is_err());
        remove.note = "Linked to the wrong client".to_owned();
        assert_eq!(
            build_change(&remove, &edge(), true, None),
            Ok(RelationshipChange::Remove {
                reason: "Linked to the wrong client".to_owned()
            })
        );
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
        state.given_name = "Jamie".to_owned();
        state.family_name = "Lee".to_owned();
        state.relationships.push(edge());

        let write = state.build_write().unwrap();
        assert!(write.details.get("relationships").is_none());
    }

    #[test]
    fn funding_forms_build_typed_payloads() {
        let mut state = ContactWindowState::new("client".to_owned());
        state.new_package_source_id = Some("source-1".to_owned());
        state.new_package_name = "NDIS plan".to_owned();
        state.new_package_start = "2026-11-01".to_owned();
        state.new_package_end = "2027-10-31".to_owned();
        state.new_package_budget = "5000".to_owned();
        let package = build_funding_package(&state).unwrap();
        assert_eq!(package.funding_source_id, "source-1");
        assert_eq!(package.budget_amount, Some(5000.0));

        state.new_agreement_package_id = Some("package-1".to_owned());
        state.new_agreement_title = "Weekly care".to_owned();
        state.new_agreement_start = "2026-11-01".to_owned();
        state.new_agreement_end = "2027-01-31".to_owned();
        let agreement = build_agreement(&state).unwrap();
        assert_eq!(agreement.status.as_deref(), Some("quoted"));

        state.new_line_description = "Personal care".to_owned();
        state.new_line_quantity = "2".to_owned();
        state.new_line_rate = "68".to_owned();
        state.new_line_recurrence = "weekly".to_owned();
        state.new_line_weekdays = vec![1, 3];
        state.new_line_start_time = "09:00".to_owned();
        state.new_line_duration = "120".to_owned();
        let line = build_agreement_line(&state, "agreement-1").unwrap();
        assert_eq!(line.start_time.as_deref(), Some("09:00:00"));
        assert_eq!(line.weekdays, vec![1, 3]);
        assert_eq!(line.duration_minutes, Some(120));

        state.new_package_start = "01/11/2026".to_owned();
        assert!(build_funding_package(&state).is_err());
    }
}
