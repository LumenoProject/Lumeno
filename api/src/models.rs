use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Clone, Debug, FromRow)]
pub struct SessionUser {
    pub id: Uuid,
    pub display_name: String,
    pub email: Option<String>,
    pub timezone: String,
    pub csrf_hash: Vec<u8>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeResponse {
    pub id: Uuid,
    pub display_name: String,
    pub email: Option<String>,
    pub timezone: String,
    pub csrf_token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsRequest {
    pub timezone: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, sqlx::Type)]
#[sqlx(type_name = "chain_kind", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum ChainKind {
    Unit,
    Group,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, sqlx::Type)]
#[sqlx(type_name = "unit_kind", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum UnitKind {
    Basic,
    Assault,
    Recon,
    Command,
    SpecialOps,
    Engineering,
    Quartermaster,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chain {
    pub id: Uuid,
    pub logical_id: Uuid,
    pub revision: i32,
    pub previous_revision_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub kind: ChainKind,
    pub unit_kind: Option<UnitKind>,
    pub name: String,
    pub description: String,
    pub position: i32,
    pub sacred_trigger: Option<String>,
    pub duration_seconds: Option<i32>,
    pub auxiliary_signal: Option<String>,
    pub auxiliary_delay_seconds: Option<i32>,
    pub auxiliary_completion_trigger: Option<String>,
    pub group_time_limit_seconds: Option<i32>,
    pub main_streak: i32,
    pub auxiliary_streak: i32,
    pub total_completions: i64,
    pub total_failures: i64,
    pub auxiliary_failures: i64,
    pub version: i64,
    pub archived_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChainRequest {
    pub parent_id: Option<Uuid>,
    pub kind: ChainKind,
    pub unit_kind: Option<UnitKind>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub position: i32,
    pub sacred_trigger: Option<String>,
    pub duration_seconds: Option<i32>,
    pub auxiliary_signal: Option<String>,
    pub auxiliary_delay_seconds: Option<i32>,
    pub auxiliary_completion_trigger: Option<String>,
    pub group_time_limit_seconds: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateChainRequest {
    pub version: i64,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub description: String,
    pub position: i32,
    pub sacred_trigger: Option<String>,
    pub duration_seconds: Option<i32>,
    pub auxiliary_signal: Option<String>,
    pub auxiliary_delay_seconds: Option<i32>,
    pub auxiliary_completion_trigger: Option<String>,
    pub group_time_limit_seconds: Option<i32>,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Precedent {
    pub id: Uuid,
    pub chain_id: Uuid,
    pub scope: String,
    pub rule: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Booking {
    pub id: Uuid,
    pub chain_id: Uuid,
    pub state: String,
    pub scheduled_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub confirmation_expires_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub version: i64,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct FocusSession {
    pub id: Uuid,
    pub chain_id: Uuid,
    pub booking_id: Option<Uuid>,
    pub group_run_id: Option<Uuid>,
    pub state: String,
    pub started_at: DateTime<Utc>,
    pub completion_available_at: Option<DateTime<Utc>>,
    pub pending_action: Option<String>,
    pub pending_reason: Option<String>,
    pub version: i64,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct GroupRun {
    pub id: Uuid,
    pub chain_id: Uuid,
    pub state: String,
    pub started_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub version: i64,
    pub next_chain_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartFocusRequest {
    pub booking_id: Option<Uuid>,
    pub group_run_id: Option<Uuid>,
    pub client_session_id: Option<Uuid>,
    pub occurred_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RaiseJudgmentRequest {
    pub action: String,
    pub reason: String,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveJudgmentRequest {
    pub decision: String,
    pub precedent_id: Option<Uuid>,
    pub new_rule: Option<String>,
    pub version: i64,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolEvent {
    pub id: Uuid,
    pub chain_id: Option<Uuid>,
    pub aggregate_type: String,
    pub aggregate_id: Uuid,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub occurred_at: DateTime<Utc>,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CommitmentRecord {
    pub completions: i64,
    pub failures: i64,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NegativeState {
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub cues: String,
    pub consequences: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateNegativeStateRequest {
    pub title: String,
    pub description: String,
    pub cues: String,
    pub consequences: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TracebackStepInput {
    pub decision: String,
    pub is_intervention_point: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRsipPolicyRequest {
    pub negative_state_id: Uuid,
    pub steps: Vec<TracebackStepInput>,
    pub mode: RsipPolicyMode,
    pub title: String,
    pub trigger_condition: String,
    pub required_action: String,
    pub intended_prevention: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, sqlx::Type)]
#[sqlx(type_name = "rsip_policy_mode", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum RsipPolicyMode {
    Active,
    Passive,
    Anchored,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRsipPolicyRequest {
    pub version: i64,
    pub mode: RsipPolicyMode,
    pub title: String,
    pub trigger_condition: String,
    pub required_action: String,
    pub intended_prevention: String,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RsipPolicy {
    pub id: Uuid,
    pub revision_id: Uuid,
    pub revision: i32,
    pub negative_state_id: Option<Uuid>,
    pub intervention_step_id: Option<Uuid>,
    pub mode: RsipPolicyMode,
    pub title: String,
    pub trigger_condition: String,
    pub required_action: String,
    pub intended_prevention: String,
    pub total_active_seconds: i64,
    pub deployment_count: i64,
    pub archived_at: Option<DateTime<Utc>>,
    pub version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRsipTrialRequest {
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RsipTrial {
    pub id: Uuid,
    pub policy_id: Uuid,
    pub policy_revision_id: Uuid,
    pub succeeded_on: NaiveDate,
    pub succeeded_at: DateTime<Utc>,
    pub note: String,
    pub consumed_node_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployRsipPolicyRequest {
    pub trial_id: Uuid,
    pub parent_node_id: Option<Uuid>,
    pub group_node_id: Option<Uuid>,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RsipTreeNode {
    pub id: Uuid,
    pub kind: String,
    pub title: String,
    pub policy_mode: Option<RsipPolicyMode>,
    pub trigger_condition: Option<String>,
    pub required_action: Option<String>,
    pub policy_id: Option<Uuid>,
    pub policy_revision_id: Option<Uuid>,
    pub group_id: Option<Uuid>,
    pub parent_node_id: Option<Uuid>,
    pub state: String,
    pub deployed_on: Option<NaiveDate>,
    pub deployed_at: DateTime<Utc>,
    pub extinguished_at: Option<DateTime<Utc>>,
    pub failure_reason: Option<String>,
    pub group_ever_effective: bool,
    pub version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRsipGroupRequest {
    pub name: String,
    pub target_member_count: i32,
    pub minimum_active_members: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRsipGroupRequest {
    pub version: i64,
    pub name: String,
    pub target_member_count: i32,
    pub minimum_active_members: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployRsipGroupRequest {
    pub parent_node_id: Option<Uuid>,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RsipGroup {
    pub id: Uuid,
    pub name: String,
    pub target_member_count: i32,
    pub minimum_active_members: i32,
    pub archived_at: Option<DateTime<Utc>>,
    pub version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RsipGroupMember {
    pub group_node_id: Uuid,
    pub policy_node_id: Uuid,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RsipOverview {
    pub local_date: NaiveDate,
    pub can_deploy_today: bool,
    pub policies: Vec<RsipPolicy>,
    pub trials: Vec<RsipTrial>,
    pub groups: Vec<RsipGroup>,
    pub nodes: Vec<RsipTreeNode>,
    pub memberships: Vec<RsipGroupMember>,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RsipTodayTrial {
    pub id: Uuid,
    pub policy_id: Uuid,
    pub title: String,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RsipTodayGroup {
    pub node_id: Uuid,
    pub name: String,
    pub active_members: i64,
    pub minimum_active_members: i32,
    pub target_member_count: i32,
    pub ever_effective: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RsipToday {
    pub local_date: NaiveDate,
    pub can_deploy_today: bool,
    pub pending_trials: Vec<RsipTodayTrial>,
    pub groups: Vec<RsipTodayGroup>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailRsipDeploymentRequest {
    pub version: i64,
    pub reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackPreview {
    pub root_id: Uuid,
    pub affected: Vec<RsipNodeSummary>,
    pub affected_count: i64,
    pub truncated: bool,
    pub group_remains_effective: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RsipNodeSummary {
    pub id: Uuid,
    pub title: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RsipHistoryPage {
    pub items: Vec<RsipTreeNode>,
    pub next_cursor: Option<Uuid>,
}
