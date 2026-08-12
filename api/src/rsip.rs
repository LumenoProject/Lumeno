use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post, put},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::Utc;
use chrono_tz::Tz;
use serde::Deserialize;
use serde_json::json;
use sqlx::{AssertSqlSafe, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    AppState, auth, domain,
    error::{ApiError, ApiResult},
    models::{
        CreateNegativeStateRequest, CreateRsipGroupRequest, CreateRsipPolicyRequest,
        CreateRsipTrialRequest, DeployRsipGroupRequest, DeployRsipPolicyRequest,
        FailRsipDeploymentRequest, NegativeState, RollbackPreview, RsipGroup, RsipGroupMember,
        RsipHistoryPage, RsipNodeSummary, RsipOverview, RsipPolicy, RsipToday, RsipTodayGroup,
        RsipTodayTrial, RsipTreeNode, RsipTrial, TracebackStepInput, UpdateRsipGroupRequest,
        UpdateRsipPolicyRequest,
    },
};

const POLICY_SELECT: &str = "SELECT p.id,r.id AS revision_id,r.revision,r.negative_state_id,r.intervention_step_id,\
     r.mode,r.title,r.trigger_condition,r.required_action,r.intended_prevention,\
     p.total_active_seconds+COALESCE((\
       SELECT sum(GREATEST(0,EXTRACT(EPOCH FROM (now()-n.deployed_at))))::bigint \
       FROM rsip_tree_nodes n WHERE n.policy_id=p.id AND n.user_id=p.user_id AND n.state='active'\
     ),0) AS total_active_seconds,\
     p.deployment_count,p.archived_at,p.version,p.created_at,p.updated_at \
     FROM rsip_policies p JOIN rsip_policy_revisions r \
       ON r.policy_id=p.id AND r.revision=p.current_revision";

const NODE_SELECT: &str = "SELECT n.id,n.kind::text AS kind,COALESCE(r.title,g.name) AS title,r.mode AS policy_mode,\
     r.trigger_condition,r.required_action,\
     n.policy_id,n.policy_revision_id,n.group_id,n.parent_node_id,n.state::text AS state,\
     n.deployed_on,n.deployed_at,n.extinguished_at,n.failure_reason,n.group_ever_effective,n.version \
     FROM rsip_tree_nodes n \
     LEFT JOIN rsip_policy_revisions r ON r.id=n.policy_revision_id \
     LEFT JOIN rsip_groups g ON g.id=n.group_id";

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/rsip/negative-states",
            get(list_negative_states).post(create_negative_state),
        )
        .route("/api/v1/rsip/overview", get(overview))
        .route("/api/v1/rsip/today", get(today))
        .route("/api/v1/rsip/history", get(history))
        .route("/api/v1/rsip/policies", post(create_policy))
        .route(
            "/api/v1/rsip/policies/{id}",
            put(update_policy).delete(archive_policy),
        )
        .route("/api/v1/rsip/policies/{id}/trials", post(create_trial))
        .route("/api/v1/rsip/policies/{id}/deploy", post(deploy_policy))
        .route("/api/v1/rsip/groups", post(create_group))
        .route(
            "/api/v1/rsip/groups/{id}",
            put(update_group).delete(archive_group),
        )
        .route("/api/v1/rsip/groups/{id}/deploy", post(deploy_group))
        .route(
            "/api/v1/rsip/deployments/{id}/rollback-preview",
            get(rollback_preview),
        )
        .route("/api/v1/rsip/deployments/{id}/fail", post(fail_deployment))
}

async fn list_negative_states(
    State(state): State<AppState>,
    jar: CookieJar,
) -> ApiResult<Json<Vec<NegativeState>>> {
    let user = auth::require_user(&state.pool, &jar, None, false).await?;
    Ok(Json(
        sqlx::query_as::<_, NegativeState>(
            "SELECT id,title,description,cues,consequences,created_at,updated_at \
             FROM negative_states WHERE user_id=$1 ORDER BY updated_at DESC LIMIT 500",
        )
        .bind(user.id)
        .fetch_all(&state.pool)
        .await?,
    ))
}

async fn create_negative_state(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(request): Json<CreateNegativeStateRequest>,
) -> ApiResult<(StatusCode, Json<NegativeState>)> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    if request.title.trim().is_empty() {
        return Err(ApiError::validation("不可逃逸区名称不能为空"));
    }
    if request.title.chars().count() > 160
        || request.description.chars().count() > 4000
        || request.cues.chars().count() > 4000
        || request.consequences.chars().count() > 4000
    {
        return Err(ApiError::validation("不可逃逸区字段超出长度限制"));
    }
    let mut tx = state.pool.begin().await?;
    lock_rsip_mutations(&mut tx, user.id).await?;
    let row = sqlx::query_as::<_, NegativeState>(
        "INSERT INTO negative_states(id,user_id,title,description,cues,consequences) \
         VALUES($1,$2,$3,$4,$5,$6) \
         RETURNING id,title,description,cues,consequences,created_at,updated_at",
    )
    .bind(Uuid::now_v7())
    .bind(user.id)
    .bind(request.title.trim())
    .bind(request.description.trim())
    .bind(request.cues.trim())
    .bind(request.consequences.trim())
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn overview(State(state): State<AppState>, jar: CookieJar) -> ApiResult<Json<RsipOverview>> {
    let user = auth::require_user(&state.pool, &jar, None, false).await?;
    let local_date = user_local_date(&user.timezone)?;
    let policies = sqlx::query_as::<_, RsipPolicy>(AssertSqlSafe(format!(
        "{POLICY_SELECT} WHERE p.user_id=$1 ORDER BY p.archived_at NULLS FIRST,p.updated_at DESC LIMIT 1000"
    )))
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;
    let trials = sqlx::query_as::<_, RsipTrial>(
        "SELECT id,policy_id,policy_revision_id,succeeded_on,succeeded_at,note,consumed_node_id \
         FROM rsip_trials WHERE user_id=$1 AND succeeded_on=$2 \
         ORDER BY succeeded_at DESC LIMIT 1000",
    )
    .bind(user.id)
    .bind(local_date)
    .fetch_all(&state.pool)
    .await?;
    let groups = sqlx::query_as::<_, RsipGroup>(
        "SELECT id,name,target_member_count,minimum_active_members,archived_at,version,created_at,updated_at \
         FROM rsip_groups WHERE user_id=$1 ORDER BY archived_at NULLS FIRST,updated_at DESC LIMIT 500",
    )
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;
    let nodes = sqlx::query_as::<_, RsipTreeNode>(AssertSqlSafe(format!(
        "{NODE_SELECT} WHERE n.user_id=$1 AND n.state='active' ORDER BY n.deployed_at,n.id LIMIT 1000"
    )))
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;
    let memberships = sqlx::query_as::<_, RsipGroupMember>(
        "SELECT m.group_node_id,m.policy_node_id FROM rsip_group_members m \
         JOIN rsip_tree_nodes g ON g.id=m.group_node_id \
         JOIN rsip_tree_nodes p ON p.id=m.policy_node_id \
         WHERE m.user_id=$1 AND g.state='active' AND p.state='active' LIMIT 1000",
    )
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;
    let deployed_today: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM rsip_tree_nodes WHERE user_id=$1 AND deployed_on=$2)",
    )
    .bind(user.id)
    .bind(local_date)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(RsipOverview {
        local_date,
        can_deploy_today: !deployed_today,
        policies,
        trials,
        groups,
        nodes,
        memberships,
    }))
}

async fn today(State(state): State<AppState>, jar: CookieJar) -> ApiResult<Json<RsipToday>> {
    let user = auth::require_user(&state.pool, &jar, None, false).await?;
    let local_date = user_local_date(&user.timezone)?;
    let deployed_today: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM rsip_tree_nodes WHERE user_id=$1 AND deployed_on=$2)",
    )
    .bind(user.id)
    .bind(local_date)
    .fetch_one(&state.pool)
    .await?;
    let pending_trials = sqlx::query_as::<_, RsipTodayTrial>(
        "SELECT t.id,t.policy_id,r.title FROM rsip_trials t \
         JOIN rsip_policies p ON p.id=t.policy_id AND p.user_id=t.user_id \
         JOIN rsip_policy_revisions r ON r.id=t.policy_revision_id \
          AND r.policy_id=p.id AND r.revision=p.current_revision \
         WHERE t.user_id=$1 AND t.succeeded_on=$2 AND t.consumed_node_id IS NULL \
           AND p.archived_at IS NULL AND NOT EXISTS(\
             SELECT 1 FROM rsip_tree_nodes n \
             WHERE n.user_id=t.user_id AND n.policy_id=t.policy_id AND n.state='active'\
           ) ORDER BY t.succeeded_at LIMIT 100",
    )
    .bind(user.id)
    .bind(local_date)
    .fetch_all(&state.pool)
    .await?;
    let groups = sqlx::query_as::<_, RsipTodayGroup>(
        "SELECT n.id AS node_id,g.name,\
           count(member.id) FILTER (WHERE member.state='active') AS active_members,\
           g.minimum_active_members,g.target_member_count,n.group_ever_effective AS ever_effective \
         FROM rsip_tree_nodes n JOIN rsip_groups g ON g.id=n.group_id AND g.user_id=n.user_id \
         LEFT JOIN rsip_group_members m ON m.group_node_id=n.id AND m.user_id=n.user_id \
         LEFT JOIN rsip_tree_nodes member ON member.id=m.policy_node_id AND member.user_id=n.user_id \
         WHERE n.user_id=$1 AND n.kind='group' AND n.state='active' \
         GROUP BY n.id,g.name,g.minimum_active_members,g.target_member_count,n.group_ever_effective \
         HAVING count(member.id) FILTER (WHERE member.state='active') < g.minimum_active_members \
         ORDER BY n.deployed_at LIMIT 100",
    )
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(RsipToday {
        local_date,
        can_deploy_today: !deployed_today,
        pending_trials,
        groups,
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryQuery {
    cursor: Option<Uuid>,
    limit: Option<i64>,
}

async fn history(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<HistoryQuery>,
) -> ApiResult<Json<RsipHistoryPage>> {
    let user = auth::require_user(&state.pool, &jar, None, false).await?;
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let rows = sqlx::query_as::<_, RsipTreeNode>(AssertSqlSafe(format!(
        "{NODE_SELECT} WHERE n.user_id=$1 AND n.state='extinguished' \
         AND ($2::uuid IS NULL OR (n.extinguished_at,n.id) < (\
           SELECT c.extinguished_at,c.id FROM rsip_tree_nodes c WHERE c.id=$2 AND c.user_id=$1\
         )) ORDER BY n.extinguished_at DESC,n.id DESC LIMIT $3"
    )))
    .bind(user.id)
    .bind(query.cursor)
    .bind(limit + 1)
    .fetch_all(&state.pool)
    .await?;
    let has_more = rows.len() > usize::try_from(limit).unwrap_or(100);
    let mut items = rows;
    items.truncate(usize::try_from(limit).unwrap_or(100));
    let next_cursor = has_more.then(|| items.last().map(|item| item.id)).flatten();
    Ok(Json(RsipHistoryPage { items, next_cursor }))
}

async fn create_policy(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(request): Json<CreateRsipPolicyRequest>,
) -> ApiResult<(StatusCode, Json<RsipPolicy>)> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    validate_policy_fields(
        &request.title,
        &request.trigger_condition,
        &request.required_action,
        &request.intended_prevention,
    )?;
    validate_traceback(&request.steps)?;
    let mut tx = state.pool.begin().await?;
    lock_rsip_mutations(&mut tx, user.id).await?;
    ensure_negative_state(&mut tx, user.id, request.negative_state_id).await?;
    let intervention =
        insert_traceback(&mut tx, user.id, request.negative_state_id, request.steps).await?;
    let policy_id = Uuid::now_v7();
    let revision_id = Uuid::now_v7();
    sqlx::query("INSERT INTO rsip_policies(id,user_id) VALUES($1,$2)")
        .bind(policy_id)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO rsip_policy_revisions(\
           id,policy_id,user_id,revision,negative_state_id,intervention_step_id,mode,title,\
           trigger_condition,required_action,intended_prevention\
         ) VALUES($1,$2,$3,1,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(revision_id)
    .bind(policy_id)
    .bind(user.id)
    .bind(request.negative_state_id)
    .bind(intervention)
    .bind(request.mode)
    .bind(request.title.trim())
    .bind(request.trigger_condition.trim())
    .bind(request.required_action.trim())
    .bind(request.intended_prevention.trim())
    .execute(&mut *tx)
    .await?;
    insert_rsip_event(
        &mut tx,
        user.id,
        Some(policy_id),
        None,
        None,
        "policy_created",
        json!({"revision": 1}),
    )
    .await?;
    let row = fetch_policy_tx(&mut tx, user.id, policy_id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn update_policy(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateRsipPolicyRequest>,
) -> ApiResult<Json<RsipPolicy>> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    validate_policy_fields(
        &request.title,
        &request.trigger_condition,
        &request.required_action,
        &request.intended_prevention,
    )?;
    let mut tx = state.pool.begin().await?;
    lock_rsip_mutations(&mut tx, user.id).await?;
    let current = fetch_policy_tx(&mut tx, user.id, id).await?;
    if current.archived_at.is_some() || current.version != request.version {
        return Err(ApiError::Conflict("国策已归档或版本已改变".to_owned()));
    }
    let has_history: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM rsip_trials WHERE policy_revision_id=$1) \
           OR EXISTS(SELECT 1 FROM rsip_tree_nodes WHERE policy_revision_id=$1)",
    )
    .bind(current.revision_id)
    .fetch_one(&mut *tx)
    .await?;
    if has_history {
        let revision_id = Uuid::now_v7();
        let revision = current.revision + 1;
        sqlx::query(
            "INSERT INTO rsip_policy_revisions(\
               id,policy_id,user_id,revision,previous_revision_id,negative_state_id,\
               intervention_step_id,mode,title,trigger_condition,required_action,intended_prevention\
             ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(revision_id)
        .bind(id)
        .bind(user.id)
        .bind(revision)
        .bind(current.revision_id)
        .bind(current.negative_state_id)
        .bind(current.intervention_step_id)
        .bind(request.mode)
        .bind(request.title.trim())
        .bind(request.trigger_condition.trim())
        .bind(request.required_action.trim())
        .bind(request.intended_prevention.trim())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE rsip_policies SET current_revision=$1,version=version+1,updated_at=now() \
             WHERE id=$2 AND user_id=$3",
        )
        .bind(revision)
        .bind(id)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
        insert_rsip_event(
            &mut tx,
            user.id,
            Some(id),
            None,
            None,
            "policy_revised",
            json!({"revision": revision}),
        )
        .await?;
    } else {
        sqlx::query(
            "UPDATE rsip_policy_revisions SET mode=$1,title=$2,trigger_condition=$3,\
             required_action=$4,intended_prevention=$5 WHERE id=$6 AND user_id=$7",
        )
        .bind(request.mode)
        .bind(request.title.trim())
        .bind(request.trigger_condition.trim())
        .bind(request.required_action.trim())
        .bind(request.intended_prevention.trim())
        .bind(current.revision_id)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE rsip_policies SET version=version+1,updated_at=now() WHERE id=$1 AND user_id=$2",
        )
        .bind(id)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    }
    let row = fetch_policy_tx(&mut tx, user.id, id).await?;
    tx.commit().await?;
    Ok(Json(row))
}

async fn archive_policy(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    let mut tx = state.pool.begin().await?;
    lock_rsip_mutations(&mut tx, user.id).await?;
    let updated = sqlx::query(
        "UPDATE rsip_policies p SET archived_at=now(),version=version+1,updated_at=now() \
         WHERE p.id=$1 AND p.user_id=$2 AND p.archived_at IS NULL \
           AND NOT EXISTS(SELECT 1 FROM rsip_tree_nodes n \
                          WHERE n.policy_id=p.id AND n.state='active')",
    )
    .bind(id)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(ApiError::Conflict("活动国策不能归档".to_owned()));
    }
    insert_rsip_event(
        &mut tx,
        user.id,
        Some(id),
        None,
        None,
        "policy_archived",
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_trial(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<CreateRsipTrialRequest>,
) -> ApiResult<(StatusCode, Json<RsipTrial>)> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    if request.note.chars().count() > 1000 {
        return Err(ApiError::validation("试行说明超出长度限制"));
    }
    let local_date = user_local_date(&user.timezone)?;
    let mut tx = state.pool.begin().await?;
    lock_rsip_mutations(&mut tx, user.id).await?;
    let policy = fetch_policy_tx(&mut tx, user.id, id).await?;
    if policy.archived_at.is_some() {
        return Err(ApiError::Conflict("已归档国策不能试行".to_owned()));
    }
    let trial_id = Uuid::now_v7();
    let trial = sqlx::query_as::<_, RsipTrial>(
        "INSERT INTO rsip_trials(\
           id,user_id,policy_id,policy_revision_id,succeeded_on,note\
         ) VALUES($1,$2,$3,$4,$5,$6) \
         RETURNING id,policy_id,policy_revision_id,succeeded_on,succeeded_at,note,consumed_node_id",
    )
    .bind(trial_id)
    .bind(user.id)
    .bind(id)
    .bind(policy.revision_id)
    .bind(local_date)
    .bind(request.note.trim())
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| match &error {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            ApiError::Conflict("今天已经记录过该国策的成功试行".to_owned())
        }
        _ => ApiError::Database(error),
    })?;
    insert_rsip_event(
        &mut tx,
        user.id,
        Some(id),
        None,
        None,
        "trial_succeeded",
        json!({"date": local_date}),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(trial)))
}

async fn deploy_policy(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<DeployRsipPolicyRequest>,
) -> ApiResult<(StatusCode, Json<RsipTreeNode>)> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    if request.parent_node_id.is_some() && request.group_node_id.is_some() {
        return Err(ApiError::validation("国策不能同时作为组成员和普通依赖"));
    }
    let local_date = user_local_date(&user.timezone)?;
    let mut tx = state.pool.begin().await?;
    lock_rsip_mutations(&mut tx, user.id).await?;
    let trial: Option<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT t.policy_revision_id,r.id FROM rsip_trials t \
         JOIN rsip_policies p ON p.id=t.policy_id \
         JOIN rsip_policy_revisions r ON r.policy_id=p.id AND r.revision=p.current_revision \
         WHERE t.id=$1 AND t.user_id=$2 AND t.policy_id=$3 AND t.succeeded_on=$4 \
           AND t.consumed_node_id IS NULL AND p.archived_at IS NULL \
           AND t.policy_revision_id=r.id FOR UPDATE OF t,p",
    )
    .bind(request.trial_id)
    .bind(user.id)
    .bind(id)
    .bind(local_date)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((revision_id, _)) = trial else {
        return Err(ApiError::Conflict(
            "试行记录已失效、已消费或不属于当前修订".to_owned(),
        ));
    };
    if let Some(parent) = request.parent_node_id {
        ensure_effective_parent(&mut tx, user.id, parent).await?;
    }
    if let Some(group_node) = request.group_node_id {
        ensure_group_has_capacity(&mut tx, user.id, group_node).await?;
    }
    let node_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO rsip_tree_nodes(\
           id,user_id,kind,policy_id,policy_revision_id,parent_node_id,deployed_on\
         ) VALUES($1,$2,'policy',$3,$4,$5,$6)",
    )
    .bind(node_id)
    .bind(user.id)
    .bind(id)
    .bind(revision_id)
    .bind(request.parent_node_id)
    .bind(local_date)
    .execute(&mut *tx)
    .await
    .map_err(map_deployment_conflict)?;
    if let Some(group_node) = request.group_node_id {
        sqlx::query(
            "INSERT INTO rsip_group_members(group_node_id,policy_node_id,user_id) VALUES($1,$2,$3)",
        )
        .bind(group_node)
        .bind(node_id)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
        refresh_group_effectiveness(&mut tx, user.id, group_node).await?;
    }
    sqlx::query("UPDATE rsip_trials SET consumed_node_id=$1 WHERE id=$2")
        .bind(node_id)
        .bind(request.trial_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE rsip_policies SET deployment_count=deployment_count+1,updated_at=now() \
         WHERE id=$1 AND user_id=$2",
    )
    .bind(id)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    insert_rsip_event(
        &mut tx,
        user.id,
        Some(id),
        Some(node_id),
        None,
        "policy_deployed",
        json!({"date": local_date, "groupNodeId": request.group_node_id}),
    )
    .await?;
    let node = fetch_node_tx(&mut tx, user.id, node_id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(node)))
}

async fn create_group(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(request): Json<CreateRsipGroupRequest>,
) -> ApiResult<(StatusCode, Json<RsipGroup>)> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    validate_group(
        &request.name,
        request.target_member_count,
        request.minimum_active_members,
    )?;
    let mut tx = state.pool.begin().await?;
    lock_rsip_mutations(&mut tx, user.id).await?;
    let group_id = Uuid::now_v7();
    let row = sqlx::query_as::<_, RsipGroup>(
        "INSERT INTO rsip_groups(id,user_id,name,target_member_count,minimum_active_members) \
         VALUES($1,$2,$3,$4,$5) \
         RETURNING id,name,target_member_count,minimum_active_members,archived_at,version,created_at,updated_at",
    )
    .bind(group_id)
    .bind(user.id)
    .bind(request.name.trim())
    .bind(request.target_member_count)
    .bind(request.minimum_active_members)
    .fetch_one(&mut *tx)
    .await?;
    insert_rsip_event(
        &mut tx,
        user.id,
        None,
        None,
        Some(group_id),
        "group_created",
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn update_group(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateRsipGroupRequest>,
) -> ApiResult<Json<RsipGroup>> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    validate_group(
        &request.name,
        request.target_member_count,
        request.minimum_active_members,
    )?;
    let mut tx = state.pool.begin().await?;
    lock_rsip_mutations(&mut tx, user.id).await?;
    let row = sqlx::query_as::<_, RsipGroup>(
        "UPDATE rsip_groups g SET name=$1,target_member_count=$2,minimum_active_members=$3,\
         version=version+1,updated_at=now() \
         WHERE g.id=$4 AND g.user_id=$5 AND g.version=$6 AND g.archived_at IS NULL \
           AND NOT EXISTS(SELECT 1 FROM rsip_tree_nodes WHERE group_id=g.id) \
         RETURNING id,name,target_member_count,minimum_active_members,archived_at,version,created_at,updated_at",
    )
    .bind(request.name.trim())
    .bind(request.target_member_count)
    .bind(request.minimum_active_members)
    .bind(id)
    .bind(user.id)
    .bind(request.version)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| ApiError::Conflict("已有部署历史的国策组不能修改结构".to_owned()))?;
    insert_rsip_event(
        &mut tx,
        user.id,
        None,
        None,
        Some(id),
        "group_updated",
        json!({"version": row.version}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(row))
}

async fn archive_group(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    let mut tx = state.pool.begin().await?;
    lock_rsip_mutations(&mut tx, user.id).await?;
    let result = sqlx::query(
        "UPDATE rsip_groups g SET archived_at=now(),version=version+1,updated_at=now() \
         WHERE g.id=$1 AND g.user_id=$2 AND g.archived_at IS NULL \
           AND NOT EXISTS(SELECT 1 FROM rsip_tree_nodes WHERE group_id=g.id)",
    )
    .bind(id)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() != 1 {
        return Err(ApiError::Conflict(
            "已有部署历史的国策组不能归档".to_owned(),
        ));
    }
    insert_rsip_event(
        &mut tx,
        user.id,
        None,
        None,
        Some(id),
        "group_archived",
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn deploy_group(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<DeployRsipGroupRequest>,
) -> ApiResult<(StatusCode, Json<RsipTreeNode>)> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    let mut tx = state.pool.begin().await?;
    lock_rsip_mutations(&mut tx, user.id).await?;
    let available: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM rsip_groups WHERE id=$1 AND user_id=$2 AND archived_at IS NULL)",
    )
    .bind(id)
    .bind(user.id)
    .fetch_one(&mut *tx)
    .await?;
    if !available {
        return Err(ApiError::NotFound);
    }
    if let Some(parent) = request.parent_node_id {
        ensure_effective_parent(&mut tx, user.id, parent).await?;
    }
    let node_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO rsip_tree_nodes(id,user_id,kind,group_id,parent_node_id) \
         VALUES($1,$2,'group',$3,$4)",
    )
    .bind(node_id)
    .bind(user.id)
    .bind(id)
    .bind(request.parent_node_id)
    .execute(&mut *tx)
    .await
    .map_err(map_deployment_conflict)?;
    insert_rsip_event(
        &mut tx,
        user.id,
        None,
        Some(node_id),
        Some(id),
        "group_deployed",
        json!({}),
    )
    .await?;
    let node = fetch_node_tx(&mut tx, user.id, node_id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(node)))
}

async fn rollback_preview(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<RollbackPreview>> {
    let user = auth::require_user(&state.pool, &jar, None, false).await?;
    let mut tx = state.pool.begin().await?;
    let impact = failure_impact(&mut tx, user.id, id).await?;
    let preview = impact_preview(&mut tx, user.id, id, &impact).await?;
    Ok(Json(preview))
}

async fn fail_deployment(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<FailRsipDeploymentRequest>,
) -> ApiResult<Json<RollbackPreview>> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    if request.reason.trim().is_empty() || request.reason.chars().count() > 4000 {
        return Err(ApiError::validation("必须记录失效原因，且不能超过 4000 字"));
    }
    let mut tx = state.pool.begin().await?;
    lock_rsip_mutations(&mut tx, user.id).await?;
    let version: Option<i64> = sqlx::query_scalar(
        "SELECT version FROM rsip_tree_nodes WHERE id=$1 AND user_id=$2 AND state='active' FOR UPDATE",
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?;
    if version != Some(request.version) {
        return Err(ApiError::Conflict("部署状态已改变".to_owned()));
    }
    let impact = failure_impact(&mut tx, user.id, id).await?;
    let preview = impact_preview(&mut tx, user.id, id, &impact).await?;
    apply_failure(&mut tx, user.id, id, &impact, request.reason.trim()).await?;
    insert_rsip_event(
        &mut tx,
        user.id,
        None,
        Some(id),
        None,
        "deployment_failed",
        json!({
            "reason": request.reason.trim(),
            "affectedCount": preview.affected_count,
            "groupRemainsEffective": preview.group_remains_effective,
        }),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(preview))
}

#[derive(Clone, Copy)]
pub(crate) enum FailureImpact {
    FullSubtree,
    OnlyRoot { group_remains_effective: bool },
    RootAndGroupDependents { group_node_id: Uuid },
}

pub(crate) async fn failure_impact(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    root_id: Uuid,
) -> ApiResult<FailureImpact> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM rsip_tree_nodes WHERE id=$1 AND user_id=$2 AND state='active')",
    )
    .bind(root_id)
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await?;
    if !exists {
        return Err(ApiError::NotFound);
    }
    let membership: Option<(Uuid, i64, i32)> = sqlx::query_as(
        "SELECT m.group_node_id,\
           count(*) FILTER (WHERE member.state='active'),g.minimum_active_members \
         FROM rsip_group_members m \
         JOIN rsip_tree_nodes group_node ON group_node.id=m.group_node_id \
         JOIN rsip_groups g ON g.id=group_node.group_id \
         JOIN rsip_group_members all_members ON all_members.group_node_id=m.group_node_id \
         JOIN rsip_tree_nodes member ON member.id=all_members.policy_node_id \
         WHERE m.policy_node_id=$1 AND m.user_id=$2 AND group_node.state='active' \
         GROUP BY m.group_node_id,g.minimum_active_members",
    )
    .bind(root_id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some((group_node_id, active_members, minimum)) = membership {
        if active_members > i64::from(minimum) {
            Ok(FailureImpact::OnlyRoot {
                group_remains_effective: true,
            })
        } else {
            Ok(FailureImpact::RootAndGroupDependents { group_node_id })
        }
    } else {
        Ok(FailureImpact::FullSubtree)
    }
}

pub(crate) async fn impact_preview(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    root_id: Uuid,
    impact: &FailureImpact,
) -> ApiResult<RollbackPreview> {
    let (affected, group_remains_effective) = match impact {
        FailureImpact::OnlyRoot {
            group_remains_effective,
        } => {
            let rows = sqlx::query_as::<_, (Uuid, String, i64)>(
                "SELECT n.id,r.title,1::bigint FROM rsip_tree_nodes n \
                 JOIN rsip_policy_revisions r ON r.id=n.policy_revision_id \
                 WHERE n.id=$1 AND n.user_id=$2 AND n.state='active'",
            )
            .bind(root_id)
            .bind(user_id)
            .fetch_all(&mut **tx)
            .await?;
            (rows, Some(*group_remains_effective))
        }
        FailureImpact::FullSubtree => (
            query_impact_summary(tx, user_id, root_id, None).await?,
            None,
        ),
        FailureImpact::RootAndGroupDependents { group_node_id } => (
            query_impact_summary(tx, user_id, root_id, Some(*group_node_id)).await?,
            Some(false),
        ),
    };
    let affected_count = affected.first().map_or(0, |row| row.2);
    Ok(RollbackPreview {
        root_id,
        affected: affected
            .into_iter()
            .map(|(id, title, _)| RsipNodeSummary { id, title })
            .collect(),
        affected_count,
        truncated: affected_count > 1000,
        group_remains_effective,
    })
}

async fn query_impact_summary(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    root_id: Uuid,
    dependent_group: Option<Uuid>,
) -> ApiResult<Vec<(Uuid, String, i64)>> {
    let rows = if let Some(group_node_id) = dependent_group {
        sqlx::query_as::<_, (Uuid, String, i64)>(
            "WITH RECURSIVE edges AS (\
               SELECT parent_node_id AS parent,id AS child FROM rsip_tree_nodes \
                WHERE user_id=$1 AND state='active' AND parent_node_id IS NOT NULL \
               UNION ALL \
               SELECT m.group_node_id,m.policy_node_id FROM rsip_group_members m \
               JOIN rsip_tree_nodes g ON g.id=m.group_node_id AND g.state='active' \
               JOIN rsip_tree_nodes p ON p.id=m.policy_node_id AND p.state='active' \
               WHERE m.user_id=$1\
             ), affected(id) AS (\
               SELECT $2::uuid \
               UNION \
               SELECT n.id FROM rsip_tree_nodes n \
                WHERE n.user_id=$1 AND n.state='active' AND n.parent_node_id=$3 \
               UNION \
               SELECT e.child FROM edges e JOIN affected a ON e.parent=a.id\
             ) \
             SELECT n.id,COALESCE(r.title,g.name),count(*) OVER() \
             FROM affected a JOIN rsip_tree_nodes n ON n.id=a.id \
             LEFT JOIN rsip_policy_revisions r ON r.id=n.policy_revision_id \
             LEFT JOIN rsip_groups g ON g.id=n.group_id \
             WHERE n.user_id=$1 AND n.state='active' ORDER BY n.id LIMIT 1000",
        )
        .bind(user_id)
        .bind(root_id)
        .bind(group_node_id)
        .fetch_all(&mut **tx)
        .await?
    } else {
        sqlx::query_as::<_, (Uuid, String, i64)>(
            "WITH RECURSIVE edges AS (\
               SELECT parent_node_id AS parent,id AS child FROM rsip_tree_nodes \
                WHERE user_id=$1 AND state='active' AND parent_node_id IS NOT NULL \
               UNION ALL \
               SELECT m.group_node_id,m.policy_node_id FROM rsip_group_members m \
               JOIN rsip_tree_nodes g ON g.id=m.group_node_id AND g.state='active' \
               JOIN rsip_tree_nodes p ON p.id=m.policy_node_id AND p.state='active' \
               WHERE m.user_id=$1\
             ), affected(id) AS (\
               SELECT $2::uuid \
               UNION \
               SELECT e.child FROM edges e JOIN affected a ON e.parent=a.id\
             ) \
             SELECT n.id,COALESCE(r.title,g.name),count(*) OVER() \
             FROM affected a JOIN rsip_tree_nodes n ON n.id=a.id \
             LEFT JOIN rsip_policy_revisions r ON r.id=n.policy_revision_id \
             LEFT JOIN rsip_groups g ON g.id=n.group_id \
             WHERE n.user_id=$1 AND n.state='active' ORDER BY n.id LIMIT 1000",
        )
        .bind(user_id)
        .bind(root_id)
        .fetch_all(&mut **tx)
        .await?
    };
    Ok(rows)
}

pub(crate) async fn apply_failure(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    root_id: Uuid,
    impact: &FailureImpact,
    reason: &str,
) -> ApiResult<()> {
    match impact {
        FailureImpact::OnlyRoot { .. } => {
            sqlx::query(
                "WITH durations AS (\
                   SELECT n.policy_id,GREATEST(0,EXTRACT(EPOCH FROM (now()-n.deployed_at)))::bigint AS seconds \
                   FROM rsip_tree_nodes n WHERE n.id=$2 AND n.user_id=$1 \
                     AND n.state='active' AND n.kind='policy'\
                 ), accumulated AS (\
                   UPDATE rsip_policies p SET total_active_seconds=p.total_active_seconds+d.seconds,updated_at=now() \
                   FROM durations d WHERE p.id=d.policy_id AND p.user_id=$1 RETURNING p.id\
                 ) UPDATE rsip_tree_nodes SET state='extinguished',extinguished_at=now(),\
                 failure_reason=$3,version=version+1 \
                 WHERE id=$2 AND user_id=$1 AND state='active'",
            )
            .bind(user_id)
            .bind(root_id)
            .bind(reason)
            .execute(&mut **tx)
            .await?;
        }
        FailureImpact::FullSubtree => {
            sqlx::query(
                "WITH RECURSIVE edges AS (\
                   SELECT parent_node_id AS parent,id AS child FROM rsip_tree_nodes \
                    WHERE user_id=$1 AND state='active' AND parent_node_id IS NOT NULL \
                   UNION ALL \
                   SELECT m.group_node_id,m.policy_node_id FROM rsip_group_members m \
                   JOIN rsip_tree_nodes g ON g.id=m.group_node_id AND g.state='active' \
                   JOIN rsip_tree_nodes p ON p.id=m.policy_node_id AND p.state='active' \
                   WHERE m.user_id=$1\
                 ), affected(id) AS (\
                   SELECT $2::uuid \
                   UNION SELECT e.child FROM edges e JOIN affected a ON e.parent=a.id\
                 ), durations AS (\
                   SELECT n.policy_id,COALESCE(sum(GREATEST(0,EXTRACT(EPOCH FROM (now()-n.deployed_at))))::bigint,0) AS seconds \
                   FROM rsip_tree_nodes n JOIN affected a ON a.id=n.id \
                   WHERE n.user_id=$1 AND n.state='active' AND n.kind='policy' GROUP BY n.policy_id\
                 ), accumulated AS (\
                   UPDATE rsip_policies p SET total_active_seconds=p.total_active_seconds+d.seconds,updated_at=now() \
                   FROM durations d WHERE p.id=d.policy_id AND p.user_id=$1 RETURNING p.id\
                 ) UPDATE rsip_tree_nodes SET state='extinguished',extinguished_at=now(),\
                 failure_reason=CASE WHEN id=$2 THEN $3 ELSE '上游国策失效导致级联回滚' END,\
                 version=version+1 WHERE user_id=$1 AND state='active' AND id IN(SELECT id FROM affected)",
            )
            .bind(user_id)
            .bind(root_id)
            .bind(reason)
            .execute(&mut **tx)
            .await?;
        }
        FailureImpact::RootAndGroupDependents { group_node_id } => {
            sqlx::query(
                "WITH RECURSIVE edges AS (\
                   SELECT parent_node_id AS parent,id AS child FROM rsip_tree_nodes \
                    WHERE user_id=$1 AND state='active' AND parent_node_id IS NOT NULL \
                   UNION ALL \
                   SELECT m.group_node_id,m.policy_node_id FROM rsip_group_members m \
                   JOIN rsip_tree_nodes g ON g.id=m.group_node_id AND g.state='active' \
                   JOIN rsip_tree_nodes p ON p.id=m.policy_node_id AND p.state='active' \
                   WHERE m.user_id=$1\
                 ), seeds(id) AS (\
                   SELECT $2::uuid \
                   UNION SELECT id FROM rsip_tree_nodes \
                    WHERE user_id=$1 AND state='active' AND parent_node_id=$3\
                 ), affected(id) AS (\
                   SELECT id FROM seeds \
                   UNION SELECT e.child FROM edges e JOIN affected a ON e.parent=a.id\
                 ), durations AS (\
                   SELECT n.policy_id,COALESCE(sum(GREATEST(0,EXTRACT(EPOCH FROM (now()-n.deployed_at))))::bigint,0) AS seconds \
                   FROM rsip_tree_nodes n JOIN affected a ON a.id=n.id \
                   WHERE n.user_id=$1 AND n.state='active' AND n.kind='policy' GROUP BY n.policy_id\
                 ), accumulated AS (\
                   UPDATE rsip_policies p SET total_active_seconds=p.total_active_seconds+d.seconds,updated_at=now() \
                   FROM durations d WHERE p.id=d.policy_id AND p.user_id=$1 RETURNING p.id\
                 ) UPDATE rsip_tree_nodes SET state='extinguished',extinguished_at=now(),\
                 failure_reason=CASE WHEN id=$2 THEN $4 ELSE '国策组跌破最低点亮数导致级联回滚' END,\
                 version=version+1 WHERE user_id=$1 AND state='active' AND id IN(SELECT id FROM affected)",
            )
            .bind(user_id)
            .bind(root_id)
            .bind(group_node_id)
            .bind(reason)
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}

fn validate_policy_fields(
    title: &str,
    trigger: &str,
    action: &str,
    prevention: &str,
) -> ApiResult<()> {
    if title.trim().is_empty() || trigger.trim().is_empty() || action.trim().is_empty() {
        return Err(ApiError::validation("国策名称、触发条件和规则不能为空"));
    }
    if title.chars().count() > 160
        || trigger.chars().count() > 1000
        || action.chars().count() > 1000
        || prevention.chars().count() > 4000
    {
        return Err(ApiError::validation("国策字段超出长度限制"));
    }
    Ok(())
}

fn validate_traceback(steps: &[TracebackStepInput]) -> ApiResult<()> {
    if steps.is_empty() || steps.len() > 100 {
        return Err(ApiError::validation("回溯链必须包含 1 到 100 个节点"));
    }
    if steps
        .iter()
        .filter(|step| step.is_intervention_point)
        .count()
        != 1
    {
        return Err(ApiError::validation("必须且只能标记一个有效干预节点"));
    }
    if steps
        .iter()
        .any(|step| step.decision.trim().is_empty() || step.decision.chars().count() > 1000)
    {
        return Err(ApiError::validation("回溯节点不能为空且不能超过 1000 字"));
    }
    Ok(())
}

fn validate_group(name: &str, target: i32, minimum: i32) -> ApiResult<()> {
    if name.trim().is_empty() || name.chars().count() > 160 {
        return Err(ApiError::validation("国策组名称不能为空且不能超过 160 字"));
    }
    if !(1..=50).contains(&target) || minimum < 1 || minimum > target {
        return Err(ApiError::validation(
            "国策组容量必须为 1 到 50，最低点亮数不能超过容量",
        ));
    }
    Ok(())
}

fn user_local_date(timezone: &str) -> ApiResult<chrono::NaiveDate> {
    let timezone = timezone
        .parse::<Tz>()
        .map_err(|_| ApiError::validation("用户时区无效"))?;
    Ok(domain::activation_date(Utc::now(), timezone))
}

async fn ensure_negative_state(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    id: Uuid,
) -> ApiResult<()> {
    let owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM negative_states WHERE id=$1 AND user_id=$2)",
    )
    .bind(id)
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await?;
    if !owned {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

async fn insert_traceback(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    negative_state_id: Uuid,
    steps: Vec<TracebackStepInput>,
) -> ApiResult<Uuid> {
    let traceback_id = Uuid::now_v7();
    let mut intervention = None;
    for (position, step) in steps.into_iter().enumerate() {
        let id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO traceback_steps(\
               id,user_id,negative_state_id,traceback_id,position,decision,is_intervention_point\
             ) VALUES($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(id)
        .bind(user_id)
        .bind(negative_state_id)
        .bind(traceback_id)
        .bind(i32::try_from(position).unwrap_or(i32::MAX))
        .bind(step.decision.trim())
        .bind(step.is_intervention_point)
        .execute(&mut **tx)
        .await?;
        if step.is_intervention_point {
            intervention = Some(id);
        }
    }
    intervention.ok_or_else(|| ApiError::validation("缺少有效干预节点"))
}

async fn fetch_policy_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    id: Uuid,
) -> ApiResult<RsipPolicy> {
    sqlx::query_as::<_, RsipPolicy>(AssertSqlSafe(format!(
        "{POLICY_SELECT} WHERE p.id=$1 AND p.user_id=$2"
    )))
    .bind(id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(ApiError::NotFound)
}

async fn fetch_node_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    id: Uuid,
) -> ApiResult<RsipTreeNode> {
    sqlx::query_as::<_, RsipTreeNode>(AssertSqlSafe(format!(
        "{NODE_SELECT} WHERE n.id=$1 AND n.user_id=$2"
    )))
    .bind(id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(ApiError::NotFound)
}

async fn ensure_effective_parent(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    id: Uuid,
) -> ApiResult<()> {
    let is_group_member: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM rsip_group_members WHERE policy_node_id=$1 AND user_id=$2)",
    )
    .bind(id)
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await?;
    if is_group_member {
        return Err(ApiError::validation(
            "国策组成员只能影响组状态，不能直接承载下游依赖",
        ));
    }
    let row: Option<(String, bool, i64, Option<i32>)> = sqlx::query_as(
        "SELECT n.kind::text,n.group_ever_effective,\
           count(member.id) FILTER (WHERE member.state='active'),g.minimum_active_members \
         FROM rsip_tree_nodes n LEFT JOIN rsip_groups g ON g.id=n.group_id \
         LEFT JOIN rsip_group_members m ON m.group_node_id=n.id \
         LEFT JOIN rsip_tree_nodes member ON member.id=m.policy_node_id \
         WHERE n.id=$1 AND n.user_id=$2 AND n.state='active' \
         GROUP BY n.id,n.kind,n.group_ever_effective,g.minimum_active_members",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((kind, _, active_members, minimum)) = row else {
        return Err(ApiError::validation("父节点不存在或已经熄灭"));
    };
    if kind == "group" && active_members < i64::from(minimum.unwrap_or(i32::MAX)) {
        return Err(ApiError::validation("国策组尚未达到最低点亮数"));
    }
    Ok(())
}

async fn ensure_group_has_capacity(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    id: Uuid,
) -> ApiResult<()> {
    let row: Option<(i64, i32)> = sqlx::query_as(
        "SELECT count(member.id) FILTER (WHERE member.state='active'),g.target_member_count \
         FROM rsip_tree_nodes n JOIN rsip_groups g ON g.id=n.group_id \
         LEFT JOIN rsip_group_members m ON m.group_node_id=n.id \
         LEFT JOIN rsip_tree_nodes member ON member.id=m.policy_node_id \
         WHERE n.id=$1 AND n.user_id=$2 AND n.kind='group' AND n.state='active' \
         GROUP BY n.id,g.target_member_count",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((members, target)) = row else {
        return Err(ApiError::validation("国策组不存在或已经熄灭"));
    };
    if members >= i64::from(target) {
        return Err(ApiError::Conflict("国策组已达到目标成员数".to_owned()));
    }
    Ok(())
}

async fn refresh_group_effectiveness(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    group_node_id: Uuid,
) -> ApiResult<()> {
    sqlx::query(
        "UPDATE rsip_tree_nodes n SET group_ever_effective=true,version=version+1 \
         WHERE n.id=$1 AND n.user_id=$2 AND n.kind='group' AND n.state='active' \
           AND NOT n.group_ever_effective AND (\
             SELECT count(*) FROM rsip_group_members m \
             JOIN rsip_tree_nodes member ON member.id=m.policy_node_id \
             WHERE m.group_node_id=n.id AND member.state='active'\
           ) >= (SELECT minimum_active_members FROM rsip_groups WHERE id=n.group_id)",
    )
    .bind(group_node_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn map_deployment_conflict(error: sqlx::Error) -> ApiError {
    match &error {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            ApiError::Conflict("今天已经点亮过国策，或该国策已经在树中".to_owned())
        }
        _ => ApiError::Database(error),
    }
}

async fn insert_rsip_event(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    policy_id: Option<Uuid>,
    tree_node_id: Option<Uuid>,
    group_id: Option<Uuid>,
    event_type: &str,
    payload: serde_json::Value,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO rsip_events(\
           id,user_id,policy_id,tree_node_id,group_id,event_type,payload\
         ) VALUES($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(policy_id)
    .bind(tree_node_id)
    .bind(group_id)
    .bind(event_type)
    .bind(payload)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn lock_rsip_mutations(tx: &mut Transaction<'_, Postgres>, user_id: Uuid) -> ApiResult<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 0))")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
