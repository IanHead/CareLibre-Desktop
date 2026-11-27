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
    pub flow: ApiIntakeFlow,
    pub columns: Vec<ApiIntakeColumn>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiIntakeColumn {
    pub stage: ApiStageRef,
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
