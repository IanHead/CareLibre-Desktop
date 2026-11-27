//! Server-backed intake board models.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize)]
pub struct ApiIntakeFlowList {
    pub items: Vec<ApiIntakeFlow>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiIntakeFlow {
    pub id: String,
    pub key: String,
    pub name: String,
    pub subject_kind: String,
    pub variant: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub stages: Vec<ApiIntakeStage>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiIntakeStage {
    pub id: String,
    pub key: String,
    pub name: String,
    pub kind: String,
    pub color: Option<String>,
    pub sort_order: i32,
    pub is_terminal: bool,
    pub on_enter_contact_status: Option<String>,
    pub sla_hours: Option<i32>,
    pub checklist_gate: bool,
    #[serde(default)]
    pub visible_fields: Vec<String>,
    #[serde(default)]
    pub required_fields: Vec<String>,
    #[serde(default)]
    pub actions: BTreeMap<String, bool>,
    #[serde(default)]
    pub checklist: Vec<ApiChecklistTemplateItem>,
    #[serde(default)]
    pub assignees: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiChecklistTemplateItem {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
    pub is_required: bool,
    pub sort_order: i32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiIntakeBoard {
    #[serde(default)]
    pub flows: Vec<ApiIntakeFlow>,
    pub columns: Vec<ApiIntakeColumn>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiIntakeColumn {
    pub stage: ApiStageRef,
    #[serde(default)]
    pub stage_ids: Vec<String>,
    pub count: i64,
    #[serde(default)]
    pub cards: Vec<ApiIntakeCard>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiStageRef {
    pub id: String,
    pub key: String,
    pub name: String,
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiFlowRef {
    pub id: String,
    pub key: String,
    pub name: String,
    pub subject_kind: String,
    pub variant: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiUserRef {
    pub id: String,
    pub full_name: String,
    pub email: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiDivisionRef {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiChecklistProgress {
    pub done: i64,
    pub total: i64,
    pub required_done: i64,
    pub required_total: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiIntakeCard {
    pub id: String,
    pub display_name: String,
    pub contact_id: Option<String>,
    pub flow: ApiFlowRef,
    pub stage_id: String,
    pub stage: ApiStageRef,
    pub lifecycle_status: Option<String>,
    pub division: Option<ApiDivisionRef>,
    pub assignee: Option<ApiUserRef>,
    pub priority: i16,
    pub source: String,
    pub waitlisted: bool,
    pub checklist: ApiChecklistProgress,
    pub stage_entered_at: String,
    pub stage_due_at: Option<String>,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiIntakeCaseDetail {
    pub case: ApiIntakeCase,
    pub contact: Option<ApiContactSummary>,
    pub flow: ApiIntakeFlow,
    pub stage: ApiIntakeStage,
    #[serde(default)]
    pub visible_fields: Vec<String>,
    #[serde(default)]
    pub required_fields: Vec<String>,
    #[serde(default)]
    pub checklist: Vec<ApiCaseChecklistItem>,
    #[serde(default)]
    pub allowed_transitions: Vec<ApiStageRef>,
    #[serde(default)]
    pub action_policy: BTreeMap<String, ApiActionPolicy>,
    #[serde(default)]
    pub waitlist: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiIntakeCase {
    pub id: String,
    pub display_name: String,
    pub contact_id: Option<String>,
    pub flow: ApiFlowRef,
    pub stage_id: String,
    pub stage: ApiStageRef,
    pub lifecycle_status: Option<String>,
    pub division: Option<ApiDivisionRef>,
    pub assignee: Option<ApiUserRef>,
    pub priority: i16,
    pub source: String,
    pub waitlisted: bool,
    pub checklist: ApiChecklistProgress,
    pub stage_entered_at: String,
    pub stage_due_at: Option<String>,
    pub updated_at: String,
    pub version: i32,
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub referral: Value,
    pub created_by: Option<ApiUserRef>,
    pub created_at: String,
    pub onboarded_at: Option<String>,
    pub closed_at: Option<String>,
    pub cancellation_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiContactSummary {
    pub id: String,
    pub display_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub date_of_birth: Option<String>,
    pub lifecycle_status: String,
    pub role: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiCaseChecklistItem {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
    pub is_required: bool,
    pub completed_at: Option<String>,
    pub completed_by: Option<ApiUserRef>,
    pub note: Option<String>,
    pub sort_order: i32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiActionPolicy {
    pub allowed: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiTransitionWrite {
    pub to_stage_id: String,
    pub version: i32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiIntakeFieldCatalog {
    #[serde(default)]
    pub fields: Vec<ApiIntakeField>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiIntakeField {
    pub key: String,
    pub label: String,
    pub data_type: String,
    pub section: String,
    pub source: String,
    #[serde(default)]
    pub options: Vec<String>,
    pub required_by_default: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiContactMatchList {
    #[serde(default)]
    pub candidates: Vec<ApiContactMatch>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiContactMatch {
    pub id: String,
    pub display_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub date_of_birth: Option<String>,
    pub lifecycle_status: String,
    #[serde(rename = "match")]
    pub match_kind: String,
    pub open_case: Option<Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiIntakeContactWrite {
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub preferred_name: Option<String>,
    pub pronouns: Option<String>,
    pub date_of_birth: Option<String>,
    pub gender: Option<String>,
    pub languages: Vec<String>,
    pub cultural_identity: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub notes: Option<String>,
    pub details: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiIntakeCaseCreate {
    pub flow_id: String,
    pub contact_id: Option<String>,
    pub contact: Option<ApiIntakeContactWrite>,
    pub display_name: Option<String>,
    pub source: Option<String>,
    pub data: Value,
    pub allow_duplicate_contact: bool,
    pub duplicate_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiIntakeCaseCreateResponse {
    pub case: ApiIntakeCaseDetail,
    pub contact: Option<ApiContactSummary>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiIntakeCaseUpdate {
    pub version: i32,
    pub data: Value,
    pub force: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiChecklistToggleWrite {
    pub completed: bool,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiChecklistToggleResponse {
    pub item: ApiCaseChecklistItem,
    pub checklist: ApiChecklistProgress,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiIntakeCancelWrite {
    pub version: i32,
    pub reason: String,
}
