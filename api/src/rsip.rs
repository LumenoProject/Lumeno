use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::Utc;
use chrono_tz::Tz;
use serde_json::json;
use uuid::Uuid;

use crate::{
    AppState, auth, domain,
    error::{ApiError, ApiResult},
    models::{
        ActivateRsipRequest, CreateNegativeStateRequest, CreateRsipDraftRequest, FailRsipRequest,
        NegativeState, RollbackPreview, RsipNode, RsipNodeSummary,
    },
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/rsip/negative-states",
            get(list_negative_states).post(create_negative_state),
        )
        .route("/api/v1/rsip/nodes", get(list_nodes).post(create_draft))
        .route("/api/v1/rsip/nodes/{id}/activate", post(activate_node))
        .route(
            "/api/v1/rsip/nodes/{id}/rollback-preview",
            get(rollback_preview),
        )
        .route("/api/v1/rsip/nodes/{id}/fail", post(fail_node))
}

async fn list_negative_states(
    State(state): State<AppState>,
    jar: CookieJar,
) -> ApiResult<Json<Vec<NegativeState>>> {
    let user = auth::require_user(&state.pool, &jar, None, false).await?;
    Ok(Json(
        sqlx::query_as::<_, NegativeState>(
            "SELECT id,title,description,cues,consequences,created_at,updated_at FROM negative_states WHERE user_id=$1 ORDER BY updated_at DESC LIMIT 500",
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
    let row = sqlx::query_as::<_, NegativeState>(
        "INSERT INTO negative_states(id,user_id,title,description,cues,consequences) VALUES($1,$2,$3,$4,$5,$6) RETURNING id,title,description,cues,consequences,created_at,updated_at",
    )
    .bind(Uuid::now_v7())
    .bind(user.id)
    .bind(request.title.trim())
    .bind(request.description.trim())
    .bind(request.cues.trim())
    .bind(request.consequences.trim())
    .fetch_one(&state.pool)
    .await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn list_nodes(
    State(state): State<AppState>,
    jar: CookieJar,
) -> ApiResult<Json<Vec<RsipNode>>> {
    let user = auth::require_user(&state.pool, &jar, None, false).await?;
    Ok(Json(sqlx::query_as::<_,RsipNode>(
      "SELECT id,negative_state_id,intervention_step_id,parent_id,state::text AS state,title,trigger_condition,required_action,intended_prevention,activated_on,activated_at,archived_at,failure_reason,version,created_at,updated_at FROM rsip_nodes WHERE user_id=$1 ORDER BY CASE state WHEN 'active' THEN 0 WHEN 'draft' THEN 1 ELSE 2 END,created_at DESC LIMIT 1000")
      .bind(user.id).fetch_all(&state.pool).await?))
}

async fn create_draft(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(request): Json<CreateRsipDraftRequest>,
) -> ApiResult<(StatusCode, Json<RsipNode>)> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    if request.title.trim().is_empty()
        || request.trigger_condition.trim().is_empty()
        || request.required_action.trim().is_empty()
    {
        return Err(ApiError::validation("定式名称、触发条件和必要动作不能为空"));
    }
    if request.steps.is_empty() {
        return Err(ApiError::validation("必须至少记录一个回溯节点"));
    }
    if request.steps.len() > 100
        || request.title.chars().count() > 160
        || request.trigger_condition.chars().count() > 1000
        || request.required_action.chars().count() > 1000
        || request.intended_prevention.chars().count() > 4000
        || request
            .steps
            .iter()
            .any(|step| step.decision.chars().count() > 1000)
    {
        return Err(ApiError::validation("定式或回溯链超出长度限制"));
    }
    if request
        .steps
        .iter()
        .filter(|step| step.is_intervention_point)
        .count()
        != 1
    {
        return Err(ApiError::validation("必须且只能标记一个有效干预节点"));
    }
    let mut tx = state.pool.begin().await?;
    lock_rsip_mutations(&mut tx, user.id).await?;
    let owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM negative_states WHERE id=$1 AND user_id=$2)",
    )
    .bind(request.negative_state_id)
    .bind(user.id)
    .fetch_one(&mut *tx)
    .await?;
    if !owned {
        return Err(ApiError::NotFound);
    }
    if let Some(parent) = request.parent_id {
        let active: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM rsip_nodes WHERE id=$1 AND user_id=$2 AND state='active')",
        )
        .bind(parent)
        .bind(user.id)
        .fetch_one(&mut *tx)
        .await?;
        if !active {
            return Err(ApiError::validation("父定式必须是当前活动节点"));
        }
    }
    let mut intervention = None;
    let traceback_id = Uuid::now_v7();
    for (position, step) in request.steps.into_iter().enumerate() {
        if step.decision.trim().is_empty() {
            return Err(ApiError::validation("回溯节点不能为空"));
        }
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO traceback_steps(id,user_id,negative_state_id,traceback_id,position,decision,is_intervention_point) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(id).bind(user.id).bind(request.negative_state_id).bind(traceback_id).bind(i32::try_from(position).unwrap_or(i32::MAX)).bind(step.decision.trim()).bind(step.is_intervention_point).execute(&mut *tx).await?;
        if step.is_intervention_point {
            intervention = Some(id);
        }
    }
    let id = Uuid::now_v7();
    let row=sqlx::query_as::<_,RsipNode>(
      "INSERT INTO rsip_nodes(id,user_id,negative_state_id,intervention_step_id,parent_id,title,trigger_condition,required_action,intended_prevention) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING id,negative_state_id,intervention_step_id,parent_id,state::text AS state,title,trigger_condition,required_action,intended_prevention,activated_on,activated_at,archived_at,failure_reason,version,created_at,updated_at")
      .bind(id).bind(user.id).bind(request.negative_state_id).bind(intervention).bind(request.parent_id).bind(request.title.trim()).bind(request.trigger_condition.trim()).bind(request.required_action.trim()).bind(request.intended_prevention.trim()).fetch_one(&mut *tx).await?;
    sqlx::query("INSERT INTO rsip_events(id,user_id,node_id,event_type,payload) VALUES($1,$2,$3,'draft_created',$4)").bind(Uuid::now_v7()).bind(user.id).bind(id).bind(json!({})).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn activate_node(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<ActivateRsipRequest>,
) -> ApiResult<Json<RsipNode>> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    let timezone = user
        .timezone
        .parse::<Tz>()
        .map_err(|_| ApiError::validation("用户时区无效"))?;
    let local_date = domain::activation_date(Utc::now(), timezone);
    let mut tx = state.pool.begin().await?;
    lock_rsip_mutations(&mut tx, user.id).await?;
    let row=sqlx::query_as::<_,RsipNode>(
      "UPDATE rsip_nodes n SET state='active',activated_on=$1,activated_at=now(),version=n.version+1,updated_at=now() WHERE n.id=$2 AND n.user_id=$3 AND n.state='draft' AND n.version=$4 AND (n.parent_id IS NULL OR EXISTS(SELECT 1 FROM rsip_nodes p WHERE p.id=n.parent_id AND p.user_id=$3 AND p.state='active')) RETURNING n.id,n.negative_state_id,n.intervention_step_id,n.parent_id,n.state::text AS state,n.title,n.trigger_condition,n.required_action,n.intended_prevention,n.activated_on,n.activated_at,n.archived_at,n.failure_reason,n.version,n.created_at,n.updated_at")
      .bind(local_date).bind(id).bind(user.id).bind(request.version).fetch_optional(&mut *tx).await.map_err(|error|match &error{sqlx::Error::Database(db) if db.is_unique_violation()=>ApiError::Conflict("今天已经激活过一个定式".to_owned()),_=>ApiError::Database(error)})?.ok_or_else(||ApiError::Conflict("草稿状态或父节点已改变".to_owned()))?;
    sqlx::query("INSERT INTO rsip_events(id,user_id,node_id,event_type,payload) VALUES($1,$2,$3,'activated',$4)").bind(Uuid::now_v7()).bind(user.id).bind(id).bind(json!({"date":local_date})).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(row))
}

async fn rollback_preview(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<RollbackPreview>> {
    let user = auth::require_user(&state.pool, &jar, None, false).await?;
    let affected=sqlx::query_as::<_,(Uuid,String,i64)>("WITH RECURSIVE subtree AS (SELECT id,title FROM rsip_nodes WHERE id=$1 AND user_id=$2 AND state='active' UNION ALL SELECT n.id,n.title FROM rsip_nodes n JOIN subtree s ON n.parent_id=s.id WHERE n.user_id=$2 AND n.state<>'archived') SELECT id,title,count(*) OVER() FROM subtree ORDER BY id LIMIT 1000")
      .bind(id).bind(user.id).fetch_all(&state.pool).await?;
    if affected.is_empty() {
        return Err(ApiError::NotFound);
    }
    let affected_count = affected[0].2;
    Ok(Json(RollbackPreview {
        root_id: id,
        affected: affected
            .into_iter()
            .map(|(id, title, _)| RsipNodeSummary { id, title })
            .collect(),
        affected_count,
        truncated: affected_count > 1000,
    }))
}

async fn fail_node(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<FailRsipRequest>,
) -> ApiResult<Json<RollbackPreview>> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    if request.reason.trim().is_empty() || request.reason.chars().count() > 4000 {
        return Err(ApiError::validation("必须记录失败原因"));
    }
    let mut tx = state.pool.begin().await?;
    lock_rsip_mutations(&mut tx, user.id).await?;
    let root_version: Option<i64> = sqlx::query_scalar(
        "SELECT version FROM rsip_nodes WHERE id=$1 AND user_id=$2 AND state='active' FOR UPDATE",
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?;
    if root_version != Some(request.version) {
        return Err(ApiError::Conflict("定式状态已改变".to_owned()));
    }
    let affected=sqlx::query_as::<_,(Uuid,String,i64)>("WITH RECURSIVE subtree AS (SELECT id,title FROM rsip_nodes WHERE id=$1 AND user_id=$2 AND state='active' UNION ALL SELECT n.id,n.title FROM rsip_nodes n JOIN subtree s ON n.parent_id=s.id WHERE n.user_id=$2 AND n.state<>'archived') SELECT id,title,count(*) OVER() FROM subtree ORDER BY id LIMIT 1000").bind(id).bind(user.id).fetch_all(&mut *tx).await?;
    let affected_count = affected.first().map_or(0, |row| row.2);
    sqlx::query("WITH RECURSIVE subtree AS (SELECT id FROM rsip_nodes WHERE id=$1 AND user_id=$2 AND state='active' UNION ALL SELECT n.id FROM rsip_nodes n JOIN subtree s ON n.parent_id=s.id WHERE n.user_id=$2 AND n.state<>'archived') UPDATE rsip_nodes SET state='archived',archived_at=now(),failure_reason=CASE WHEN id=$1 THEN $3 ELSE '父定式失败导致级联回滚' END,version=version+1,updated_at=now() WHERE id IN(SELECT id FROM subtree)")
      .bind(id).bind(user.id).bind(request.reason.trim()).execute(&mut *tx).await?;
    let summaries: Vec<RsipNodeSummary> = affected
        .into_iter()
        .map(|(id, title, _)| RsipNodeSummary { id, title })
        .collect();
    sqlx::query("INSERT INTO rsip_events(id,user_id,node_id,event_type,payload) VALUES($1,$2,$3,'subtree_rolled_back',$4)").bind(Uuid::now_v7()).bind(user.id).bind(id).bind(json!({"reason":request.reason,"affectedCount":affected_count})).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(RollbackPreview {
        root_id: id,
        affected: summaries,
        affected_count,
        truncated: affected_count > 1000,
    }))
}

async fn lock_rsip_mutations(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
) -> ApiResult<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 0))")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
