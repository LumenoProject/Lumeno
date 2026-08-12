use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    AppState, auth,
    domain::{self, FocusResolution},
    error::{ApiError, ApiResult},
    models::{
        Booking, Chain, ChainKind, CommitmentRecord, CreateChainRequest, FocusSession, GroupRun,
        Precedent, ProtocolEvent, RaiseJudgmentRequest, ResolveJudgmentRequest, StartFocusRequest,
        UpdateChainRequest,
    },
};

const CHAIN_COLUMNS: &str = "id, logical_id, revision, previous_revision_id, parent_id, kind, unit_kind, name, description, position, sacred_trigger, duration_seconds, auxiliary_signal, auxiliary_delay_seconds, auxiliary_completion_trigger, group_time_limit_seconds, main_streak, auxiliary_streak, total_completions, total_failures, auxiliary_failures, version, archived_at, created_at, updated_at";
const BOOKING_CONFIRMATION_WINDOW_MINUTES: i32 = 5;
const BOOKING_START_WINDOW_SECONDS: i32 = 90;
// Fulfilling a booking starts the promised focus; completing that focus is the completion.
const COMMITMENT_COMPLETION_EVENTS: &[&str] = &["focus_succeeded", "group_run_succeeded"];
const COMMITMENT_FAILURE_EVENTS: &[&str] = &[
    "focus_failed",
    "group_run_failed",
    "booking_failed",
    "booking_confirmation_timed_out",
];

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateBookingRequest {
    client_booking_id: Option<Uuid>,
    occurred_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct TerminateBookingRequest {
    version: i64,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/chains", get(list_chains).post(create_chain))
        .route("/api/v1/chains/archived", get(list_archived_chains))
        .route(
            "/api/v1/chains/{id}",
            get(get_chain).put(update_chain).delete(archive_chain),
        )
        .route("/api/v1/chains/{id}/precedents", get(list_precedents))
        .route("/api/v1/chains/{id}/bookings", post(create_booking))
        .route("/api/v1/bookings", get(list_bookings))
        .route("/api/v1/bookings/{id}/terminate", post(terminate_booking))
        .route(
            "/api/v1/bookings/{id}/judgment",
            post(resolve_booking_judgment),
        )
        .route("/api/v1/chains/{id}/focus", post(start_focus))
        .route("/api/v1/focus/active", get(active_focus))
        .route("/api/v1/focus/{id}/complete", post(complete_focus))
        .route("/api/v1/focus/{id}/judgment", post(raise_focus_judgment))
        .route(
            "/api/v1/focus/{id}/judgment/resolve",
            post(resolve_focus_judgment),
        )
        .route("/api/v1/groups/{id}/runs", post(start_group_run))
        .route("/api/v1/groups/{id}/runs/active", get(active_group_run))
        .route(
            "/api/v1/group-runs/{id}/judgment",
            post(resolve_group_judgment),
        )
        .route("/api/v1/stats/commitment-record", get(commitment_record))
        .route("/api/v1/events", get(list_events))
}

async fn list_chains(State(state): State<AppState>, jar: CookieJar) -> ApiResult<Json<Vec<Chain>>> {
    let user = auth::require_user(&state.pool, &jar, None, false).await?;
    let query = format!(
        "SELECT {CHAIN_COLUMNS} FROM chains WHERE user_id = $1 AND archived_at IS NULL ORDER BY position, created_at LIMIT 1000"
    );
    Ok(Json(
        sqlx::query_as::<_, Chain>(sqlx::AssertSqlSafe(query))
            .bind(user.id)
            .fetch_all(&state.pool)
            .await?,
    ))
}

async fn list_archived_chains(
    State(state): State<AppState>,
    jar: CookieJar,
) -> ApiResult<Json<Vec<Chain>>> {
    let user = auth::require_user(&state.pool, &jar, None, false).await?;
    Ok(Json(fetch_archived_chains(&state.pool, user.id).await?))
}

pub(crate) async fn fetch_archived_chains(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<Chain>, sqlx::Error> {
    let query = format!(
        "SELECT {CHAIN_COLUMNS} FROM chains WHERE user_id=$1 AND archived_at IS NOT NULL \
         ORDER BY archived_at DESC, id DESC LIMIT 1000"
    );
    sqlx::query_as::<_, Chain>(sqlx::AssertSqlSafe(query))
        .bind(user_id)
        .fetch_all(pool)
        .await
}

async fn get_chain(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Chain>> {
    let user = auth::require_user(&state.pool, &jar, None, false).await?;
    Ok(Json(fetch_chain(&state.pool, user.id, id).await?))
}

async fn create_chain(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(request): Json<CreateChainRequest>,
) -> ApiResult<(StatusCode, Json<Chain>)> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    validate_chain_request(&request)?;
    let mut tx = state.pool.begin().await?;
    lock_chain_mutations(&mut tx, user.id).await?;
    validate_parent(&mut tx, user.id, request.parent_id, None).await?;
    let active_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM chains WHERE user_id=$1 AND archived_at IS NULL")
            .bind(user.id)
            .fetch_one(&mut *tx)
            .await?;
    if active_count >= 1000 {
        return Err(ApiError::Conflict(
            "活动契约链已达到 1000 条上限".to_owned(),
        ));
    }
    let id = Uuid::now_v7();
    let query = format!(
        "INSERT INTO chains (id, user_id, logical_id, revision, parent_id, kind, unit_kind, name, description, position, sacred_trigger, duration_seconds, auxiliary_signal, auxiliary_delay_seconds, auxiliary_completion_trigger, group_time_limit_seconds) \
         VALUES ($1, $2, $1, 1, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) RETURNING {CHAIN_COLUMNS}"
    );
    let chain = sqlx::query_as::<_, Chain>(sqlx::AssertSqlSafe(query))
        .bind(id)
        .bind(user.id)
        .bind(request.parent_id)
        .bind(request.kind)
        .bind(request.unit_kind)
        .bind(request.name.trim())
        .bind(request.description.trim())
        .bind(request.position)
        .bind(trim_option(request.sacred_trigger))
        .bind(request.duration_seconds)
        .bind(trim_option(request.auxiliary_signal))
        .bind(request.auxiliary_delay_seconds)
        .bind(trim_option(request.auxiliary_completion_trigger))
        .bind(request.group_time_limit_seconds)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(chain)))
}

async fn update_chain(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateChainRequest>,
) -> ApiResult<Json<Chain>> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    let current = fetch_chain(&state.pool, user.id, id).await?;
    validate_chain_update(&current, &request)?;
    let core_changed = current.sacred_trigger.as_deref() != trim_ref(&request.sacred_trigger)
        || current.duration_seconds != request.duration_seconds
        || current.auxiliary_signal.as_deref() != trim_ref(&request.auxiliary_signal)
        || current.auxiliary_delay_seconds != request.auxiliary_delay_seconds
        || current.auxiliary_completion_trigger.as_deref()
            != trim_ref(&request.auxiliary_completion_trigger)
        || current.group_time_limit_seconds != request.group_time_limit_seconds;
    if core_changed {
        create_revision(&state.pool, user.id, current, request)
            .await
            .map(Json)
    } else {
        let mut tx = state.pool.begin().await?;
        lock_chain_mutations(&mut tx, user.id).await?;
        validate_parent(&mut tx, user.id, request.parent_id, Some(id)).await?;
        let query = format!(
            "UPDATE chains SET parent_id=$1, name=$2, description=$3, position=$4, version=version+1, updated_at=now() \
             WHERE id=$5 AND user_id=$6 AND version=$7 AND archived_at IS NULL RETURNING {CHAIN_COLUMNS}"
        );
        let updated = sqlx::query_as::<_, Chain>(sqlx::AssertSqlSafe(query))
            .bind(request.parent_id)
            .bind(request.name.trim())
            .bind(request.description.trim())
            .bind(request.position)
            .bind(id)
            .bind(user.id)
            .bind(request.version)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| ApiError::Conflict("链已在其他设备更新".to_owned()))?;
        tx.commit().await?;
        Ok(Json(updated))
    }
}

#[derive(Deserialize)]
struct ArchiveQuery {
    #[serde(default)]
    mode: ArchiveMode,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ArchiveMode {
    #[default]
    RejectNonEmpty,
    DetachChildren,
    ArchiveSubtree,
}

async fn archive_chain(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(query): Query<ArchiveQuery>,
) -> ApiResult<StatusCode> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    let mut tx = state.pool.begin().await?;
    lock_chain_mutations(&mut tx, user.id).await?;
    let locked_ids = sqlx::query_scalar::<_, Uuid>(
        "WITH RECURSIVE subtree AS (\
           SELECT id FROM chains WHERE id=$2 AND user_id=$1 AND archived_at IS NULL \
           UNION ALL \
           SELECT c.id FROM chains c JOIN subtree s ON c.parent_id=s.id \
           WHERE c.user_id=$1 AND c.archived_at IS NULL\
         ) SELECT c.id FROM chains c JOIN subtree s ON s.id=c.id FOR UPDATE OF c",
    )
    .bind(user.id)
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    if locked_ids.is_empty() {
        return Err(ApiError::NotFound);
    }
    let active: bool = sqlx::query_scalar(
        "WITH RECURSIVE subtree AS (\
           SELECT id FROM chains WHERE id=$2 AND user_id=$1 AND archived_at IS NULL \
           UNION ALL \
           SELECT c.id FROM chains c JOIN subtree s ON c.parent_id=s.id \
           WHERE c.user_id=$1 AND c.archived_at IS NULL\
         ) SELECT EXISTS(\
           SELECT 1 FROM focus_sessions WHERE user_id=$1 AND chain_id IN (SELECT id FROM subtree) AND state IN ('running','completion_ready','awaiting_judgment') \
           UNION ALL SELECT 1 FROM bookings WHERE user_id=$1 AND chain_id IN (SELECT id FROM subtree) AND state IN ('pending','awaiting_judgment') \
           UNION ALL SELECT 1 FROM group_runs WHERE user_id=$1 AND chain_id IN (SELECT id FROM subtree) AND state IN ('running','awaiting_judgment') \
           UNION ALL SELECT 1 FROM group_run_items i JOIN group_runs r ON r.id=i.group_run_id \
             WHERE r.user_id=$1 AND i.chain_id IN (SELECT id FROM subtree) AND i.completed_at IS NULL \
             AND r.state IN ('running','awaiting_judgment')\
         )",
    )
    .bind(user.id)
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if active {
        return Err(ApiError::Conflict(
            "活动协议结束前不能归档契约链".to_owned(),
        ));
    }
    let child_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chains WHERE user_id=$1 AND parent_id=$2 AND archived_at IS NULL",
    )
    .bind(user.id)
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if child_count > 0 {
        match query.mode {
            ArchiveMode::RejectNonEmpty => {
                return Err(ApiError::Conflict(
                    "非空任务群必须选择解除分组或归档子树".to_owned(),
                ));
            }
            ArchiveMode::DetachChildren => {
                sqlx::query("UPDATE chains SET parent_id=NULL, version=version+1, updated_at=now() WHERE user_id=$1 AND parent_id=$2 AND archived_at IS NULL")
                    .bind(user.id).bind(id).execute(&mut *tx).await?;
            }
            ArchiveMode::ArchiveSubtree => {
                sqlx::query(
                    "WITH RECURSIVE subtree AS (SELECT id FROM chains WHERE id=$1 AND user_id=$2 UNION ALL SELECT c.id FROM chains c JOIN subtree s ON c.parent_id=s.id WHERE c.user_id=$2 AND c.archived_at IS NULL) \
                     UPDATE chains SET archived_at=now(), version=version+1, updated_at=now() WHERE id IN (SELECT id FROM subtree)",
                ).bind(id).bind(user.id).execute(&mut *tx).await?;
                tx.commit().await?;
                return Ok(StatusCode::NO_CONTENT);
            }
        }
    }
    let result = sqlx::query("UPDATE chains SET archived_at=now(), version=version+1, updated_at=now() WHERE id=$1 AND user_id=$2 AND archived_at IS NULL")
        .bind(id).bind(user.id).execute(&mut *tx).await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_precedents(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<Precedent>>> {
    let user = auth::require_user(&state.pool, &jar, None, false).await?;
    let rows = sqlx::query_as::<_, Precedent>(
        "SELECT id, chain_id, scope::text AS scope, rule, created_at FROM precedents WHERE user_id=$1 AND chain_id=$2 ORDER BY created_at DESC LIMIT 500",
    ).bind(user.id).bind(id).fetch_all(&state.pool).await?;
    Ok(Json(rows))
}

async fn create_booking(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    request: Option<Json<CreateBookingRequest>>,
) -> ApiResult<(StatusCode, Json<Booking>)> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    reconcile_due_states(&state.pool).await?;
    let request = request.map(|Json(value)| value).unwrap_or_default();
    let occurred_at = request.occurred_at.unwrap_or_else(Utc::now);
    let mut tx = state.pool.begin().await?;
    let booking_id = request.client_booking_id.unwrap_or_else(Uuid::now_v7);
    if let Some(existing) = sqlx::query_as::<_, Booking>(
        "SELECT id,chain_id,state::text AS state,scheduled_at,expires_at,confirmation_expires_at,resolved_at,version \
         FROM bookings WHERE id=$1 AND user_id=$2",
    )
    .bind(booking_id)
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?
    {
        if existing.chain_id != id {
            return Err(ApiError::Conflict("预约标识已用于其他任务".to_owned()));
        }
        tx.commit().await?;
        return Ok((StatusCode::OK, Json(existing)));
    }
    domain::validate_offline_occurrence(Utc::now(), occurred_at)?;
    let chain = fetch_chain_tx(&mut tx, user.id, id).await?;
    let delay = chain
        .auxiliary_delay_seconds
        .ok_or_else(|| ApiError::validation("任务群不能直接预约"))?;
    let row = sqlx::query_as::<_, Booking>(
        "INSERT INTO bookings (id,user_id,chain_id,scheduled_at,expires_at) \
         VALUES ($1,$2,$3,$4+make_interval(secs=>$5),$4+make_interval(secs=>$5)+make_interval(secs=>$6)) \
         ON CONFLICT (id) DO NOTHING \
         RETURNING id,chain_id,state::text AS state,scheduled_at,expires_at,confirmation_expires_at,resolved_at,version",
    ).bind(booking_id).bind(user.id).bind(id).bind(occurred_at).bind(delay).bind(BOOKING_START_WINDOW_SECONDS).fetch_optional(&mut *tx).await
        .map_err(map_unique_conflict("该任务已有待处理预约"))?;
    let Some(row) = row else {
        let existing = sqlx::query_as::<_, Booking>(
            "SELECT id,chain_id,state::text AS state,scheduled_at,expires_at,confirmation_expires_at,resolved_at,version \
             FROM bookings WHERE id=$1 AND user_id=$2",
        )
        .bind(booking_id)
        .bind(user.id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(existing) = existing.filter(|booking| booking.chain_id == id) else {
            return Err(ApiError::Conflict("预约标识已被占用".to_owned()));
        };
        tx.commit().await?;
        return Ok((StatusCode::OK, Json(existing)));
    };
    enqueue_notification(
        &mut tx,
        user.id,
        NotificationSpec {
            event_key: format!("booking-ready-{booking_id}"),
            title: "预约启动窗口已开启",
            subtitle: Some(&chain.name),
            body: "预约倒计时已经结束。\n请在 90 秒内打开 Lumeno 并开始这条链。",
            path: "/",
            due: row.scheduled_at,
        },
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn list_bookings(
    State(state): State<AppState>,
    jar: CookieJar,
) -> ApiResult<Json<Vec<Booking>>> {
    let user = auth::require_user(&state.pool, &jar, None, false).await?;
    reconcile_due_states(&state.pool).await?;
    let rows = sqlx::query_as::<_, Booking>(
        "SELECT id,chain_id,state::text AS state,scheduled_at,expires_at,confirmation_expires_at,resolved_at,version FROM bookings WHERE user_id=$1 AND state IN ('pending','awaiting_judgment') ORDER BY expires_at",
    ).bind(user.id).fetch_all(&state.pool).await?;
    Ok(Json(rows))
}

async fn terminate_booking(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<TerminateBookingRequest>,
) -> ApiResult<Json<Booking>> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    reconcile_due_states(&state.pool).await?;
    let mut tx = state.pool.begin().await?;
    let booking = sqlx::query_as::<_, Booking>(
        "SELECT id,chain_id,state::text AS state,scheduled_at,expires_at,confirmation_expires_at,resolved_at,version \
         FROM bookings WHERE id=$1 AND user_id=$2 FOR UPDATE",
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiError::NotFound)?;

    if booking.state == "awaiting_judgment" {
        tx.commit().await?;
        return Ok(Json(booking));
    }
    if booking.state != "pending" || booking.version != request.version {
        return Err(ApiError::Conflict("预约状态已改变".to_owned()));
    }

    let result = sqlx::query_as::<_, Booking>(
        "UPDATE bookings SET state='awaiting_judgment',expires_at=LEAST(expires_at,now()),\
           confirmation_expires_at=LEAST(expires_at,now())+make_interval(mins=>$2),version=version+1 \
         WHERE id=$1 \
         RETURNING id,chain_id,state::text AS state,scheduled_at,expires_at,confirmation_expires_at,resolved_at,version",
    )
    .bind(id)
    .bind(BOOKING_CONFIRMATION_WINDOW_MINUTES)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE notification_jobs SET cancelled_at=now() \
         WHERE user_id=$1 AND event_key=$2 AND sent_at IS NULL AND failed_at IS NULL AND cancelled_at IS NULL",
    )
    .bind(user.id)
    .bind(format!("booking-ready-{id}"))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(result))
}

async fn start_focus(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(chain_id): Path<Uuid>,
    Json(request): Json<StartFocusRequest>,
) -> ApiResult<(StatusCode, Json<FocusSession>)> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    reconcile_due_states(&state.pool).await?;
    let mut tx = state.pool.begin().await?;
    let session_id = request.client_session_id.unwrap_or_else(Uuid::now_v7);
    if let Some(existing) = sqlx::query_as::<_, FocusSession>(
        "SELECT id,chain_id,booking_id,group_run_id,state::text AS state,started_at,completion_available_at,pending_action,pending_reason,version FROM focus_sessions WHERE id=$1 AND user_id=$2",
    )
    .bind(session_id)
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?
    {
        tx.commit().await?;
        return Ok((StatusCode::OK, Json(existing)));
    }
    let occurred_at = request.occurred_at.unwrap_or_else(Utc::now);
    domain::validate_offline_occurrence(Utc::now(), occurred_at)?;
    let chain = fetch_chain_tx(&mut tx, user.id, chain_id).await?;
    if !matches!(chain.kind, ChainKind::Unit) {
        return Err(ApiError::validation("任务群应通过运行周期启动"));
    }
    let booking_id = match request.booking_id {
        Some(id) => Some(id),
        None => sqlx::query_scalar::<_, Uuid>("SELECT id FROM bookings WHERE user_id=$1 AND chain_id=$2 AND state IN ('pending','awaiting_judgment') ORDER BY scheduled_at LIMIT 1 FOR UPDATE")
            .bind(user.id).bind(chain_id).fetch_optional(&mut *tx).await?,
    };
    if let Some(booking_id) = booking_id {
        let booking = sqlx::query_as::<
            _,
            (DateTime<Utc>, DateTime<Utc>, String, Option<DateTime<Utc>>),
        >(
            "SELECT scheduled_at,expires_at,state::text,confirmation_expires_at FROM bookings WHERE id=$1 AND user_id=$2 AND chain_id=$3 FOR UPDATE",
        )
        .bind(booking_id)
        .bind(user.id)
        .bind(chain_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((scheduled_at, expires_at, booking_state, confirmation_expires_at)) = booking
        else {
            return Err(ApiError::NotFound);
        };
        if !matches!(booking_state.as_str(), "pending" | "awaiting_judgment")
            || !domain::booking_is_timely(scheduled_at, expires_at, occurred_at)
            || (booking_state == "awaiting_judgment"
                && !domain::booking_confirmation_is_open(confirmation_expires_at, Utc::now()))
        {
            return Err(ApiError::Conflict("预约已过期或状态已改变".to_owned()));
        }
        sqlx::query(
            "UPDATE bookings SET state='fulfilled',resolved_at=now(),version=version+1 WHERE id=$1",
        )
        .bind(booking_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE notification_jobs SET cancelled_at=now() WHERE user_id=$1 AND event_key=$2 AND sent_at IS NULL AND failed_at IS NULL AND cancelled_at IS NULL")
            .bind(user.id).bind(format!("booking-ready-{booking_id}")).execute(&mut *tx).await?;
        sqlx::query("UPDATE chains SET auxiliary_streak=auxiliary_streak+1,version=version+1,updated_at=now() WHERE id=$1 AND user_id=$2")
            .bind(chain_id).bind(user.id).execute(&mut *tx).await?;
        insert_event(
            &mut tx,
            user.id,
            Some(chain_id),
            "booking",
            booking_id,
            "booking_fulfilled",
            json!({}),
            None,
            Utc::now(),
        )
        .await?;
    }
    let group_item_id = if let Some(run_id) = request.group_run_id {
        next_group_item(&mut tx, user.id, run_id, chain_id).await?
    } else {
        None
    };
    let id = session_id;
    let row=sqlx::query_as::<_,FocusSession>(
        "INSERT INTO focus_sessions (id,user_id,chain_id,booking_id,group_run_id,group_run_item_id,started_at,completion_available_at,state) \
         VALUES ($1,$2,$3,$4,$5,$6,$8,CASE WHEN $7::integer IS NULL THEN NULL ELSE $8+make_interval(secs=>$7) END,CASE WHEN $7::integer IS NOT NULL AND $8+make_interval(secs=>$7)<=now() THEN 'completion_ready'::focus_state ELSE 'running'::focus_state END) \
         RETURNING id,chain_id,booking_id,group_run_id,state::text AS state,started_at,completion_available_at,pending_action,pending_reason,version",
    ).bind(id).bind(user.id).bind(chain_id).bind(booking_id).bind(request.group_run_id).bind(group_item_id).bind(chain.duration_seconds).bind(occurred_at)
      .fetch_one(&mut *tx).await.map_err(map_unique_conflict("已有进行中的专注任务"))?;
    if let Some(due) = row.completion_available_at {
        enqueue_notification(
            &mut tx,
            user.id,
            NotificationSpec {
                event_key: format!("focus-completion-{id}"),
                title: "专注时间已到",
                subtitle: Some(&chain.name),
                body: "本次专注的整体时间已经结束。\n现在打开 Lumeno 确认完成。",
                path: "/",
                due,
            },
        )
        .await?;
    }
    insert_event(
        &mut tx,
        user.id,
        Some(chain_id),
        "focus",
        id,
        "focus_started",
        json!({}),
        idempotency(&headers),
        occurred_at,
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn active_focus(
    State(state): State<AppState>,
    jar: CookieJar,
) -> ApiResult<Json<Option<FocusSession>>> {
    let user = auth::require_user(&state.pool, &jar, None, false).await?;
    reconcile_due_states(&state.pool).await?;
    Ok(Json(sqlx::query_as::<_,FocusSession>(
      "SELECT id,chain_id,booking_id,group_run_id,state::text AS state,started_at,completion_available_at,pending_action,pending_reason,version FROM focus_sessions WHERE user_id=$1 AND state IN ('running','completion_ready','awaiting_judgment') LIMIT 1")
      .bind(user.id).fetch_optional(&state.pool).await?))
}

async fn complete_focus(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<FocusSession>> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    let mut tx = state.pool.begin().await?;
    let session = lock_focus(&mut tx, user.id, id).await?;
    if session.state == "succeeded" {
        tx.commit().await?;
        return Ok(Json(session));
    }
    let ready = session.state == "completion_ready"
        || (session.state == "running"
            && session
                .completion_available_at
                .is_none_or(|deadline| deadline <= Utc::now()));
    if !ready {
        return Err(ApiError::Conflict("任务尚未达到可完成状态".to_owned()));
    }
    succeed_focus(
        &mut tx,
        user.id,
        &session,
        idempotency(&headers),
        Utc::now(),
    )
    .await?;
    let result = fetch_focus_tx(&mut tx, user.id, id).await?;
    tx.commit().await?;
    Ok(Json(result))
}

async fn raise_focus_judgment(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(req): Json<RaiseJudgmentRequest>,
) -> ApiResult<Json<FocusSession>> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    if previously_applied(&state.pool, user.id, idempotency(&headers), id).await? {
        return Ok(Json(fetch_focus(&state.pool, user.id, id).await?));
    }
    if !matches!(req.action.as_str(), "interrupt" | "early_end") {
        return Err(ApiError::validation("action 必须是 interrupt 或 early_end"));
    }
    if req.reason.trim().is_empty() || req.reason.chars().count() > 4000 {
        return Err(ApiError::validation("必须描述疑似违规行为"));
    }
    domain::validate_offline_occurrence(Utc::now(), req.occurred_at)?;
    let mut tx = state.pool.begin().await?;
    let changed=sqlx::query("UPDATE focus_sessions SET state='awaiting_judgment',pending_action=$1,pending_reason=$2,version=version+1 WHERE id=$3 AND user_id=$4 AND state IN ('running','completion_ready')")
      .bind(&req.action).bind(req.reason.trim()).bind(id).bind(user.id).execute(&mut *tx).await?;
    if changed.rows_affected() == 0 {
        return Err(ApiError::Conflict("会话状态已改变".to_owned()));
    }
    let session = fetch_focus_tx(&mut tx, user.id, id).await?;
    insert_event(
        &mut tx,
        user.id,
        Some(session.chain_id),
        "focus",
        id,
        "judgment_requested",
        json!({"action":req.action,"reason":req.reason}),
        idempotency(&headers),
        req.occurred_at,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(session))
}

async fn resolve_focus_judgment(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(req): Json<ResolveJudgmentRequest>,
) -> ApiResult<Json<FocusSession>> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    domain::validate_offline_occurrence(Utc::now(), req.occurred_at)?;
    if previously_applied(&state.pool, user.id, idempotency(&headers), id).await? {
        return Ok(Json(fetch_focus(&state.pool, user.id, id).await?));
    }
    let mut tx = state.pool.begin().await?;
    let session = lock_focus(&mut tx, user.id, id).await?;
    if session.state != "awaiting_judgment" || session.version != req.version {
        return Err(ApiError::Conflict("判定状态已改变".to_owned()));
    }
    match domain::focus_resolution(&req.decision, session.pending_action.as_deref())? {
        FocusResolution::Fail => {
            fail_focus(
                &mut tx,
                user.id,
                &session,
                idempotency(&headers),
                req.occurred_at,
            )
            .await?
        }
        resolution @ (FocusResolution::Succeed | FocusResolution::Continue) => {
            let precedent = resolve_precedent(
                &mut tx,
                user.id,
                session.chain_id,
                "main",
                req.precedent_id,
                req.new_rule,
            )
            .await?;
            if resolution == FocusResolution::Succeed {
                succeed_focus(
                    &mut tx,
                    user.id,
                    &session,
                    idempotency(&headers),
                    req.occurred_at,
                )
                .await?;
            } else {
                sqlx::query("UPDATE focus_sessions SET state=CASE WHEN completion_available_at IS NOT NULL AND completion_available_at<=now() THEN 'completion_ready'::focus_state ELSE 'running'::focus_state END,pending_action=NULL,pending_reason=NULL,version=version+1 WHERE id=$1")
            .bind(id).execute(&mut *tx).await?;
                insert_event(
                    &mut tx,
                    user.id,
                    Some(session.chain_id),
                    "focus",
                    id,
                    "judgment_allowed",
                    json!({"precedentId":precedent}),
                    idempotency(&headers),
                    req.occurred_at,
                )
                .await?;
            }
        }
    }
    let result = fetch_focus_tx(&mut tx, user.id, id).await?;
    tx.commit().await?;
    Ok(Json(result))
}

async fn resolve_booking_judgment(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(req): Json<ResolveJudgmentRequest>,
) -> ApiResult<Json<Booking>> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    domain::validate_offline_occurrence(Utc::now(), req.occurred_at)?;
    reconcile_due_states(&state.pool).await?;
    if previously_applied(&state.pool, user.id, idempotency(&headers), id).await? {
        let booking=sqlx::query_as::<_,Booking>("SELECT id,chain_id,state::text AS state,scheduled_at,expires_at,confirmation_expires_at,resolved_at,version FROM bookings WHERE id=$1 AND user_id=$2")
            .bind(id).bind(user.id).fetch_optional(&state.pool).await?.ok_or(ApiError::NotFound)?;
        return Ok(Json(booking));
    }
    let mut tx = state.pool.begin().await?;
    let booking=sqlx::query_as::<_,Booking>("SELECT id,chain_id,state::text AS state,scheduled_at,expires_at,confirmation_expires_at,resolved_at,version FROM bookings WHERE id=$1 AND user_id=$2 FOR UPDATE")
      .bind(id).bind(user.id).fetch_optional(&mut *tx).await?.ok_or(ApiError::NotFound)?;
    if booking.state != "awaiting_judgment" || booking.version != req.version {
        return Err(ApiError::Conflict("预约判定状态已改变".to_owned()));
    }
    if !domain::booking_confirmation_is_open(booking.confirmation_expires_at, Utc::now()) {
        return Err(ApiError::Conflict("预约确认时间已结束".to_owned()));
    }
    match req.decision.as_str() {
        "failure" => {
            sqlx::query("UPDATE bookings SET state='failed',resolved_at=now(),version=version+1 WHERE id=$1").bind(id).execute(&mut *tx).await?;
            sqlx::query("UPDATE chains SET auxiliary_streak=0,auxiliary_failures=auxiliary_failures+1,version=version+1,updated_at=now() WHERE id=$1 AND user_id=$2")
          .bind(booking.chain_id).bind(user.id).execute(&mut *tx).await?;
            insert_event(
                &mut tx,
                user.id,
                Some(booking.chain_id),
                "booking",
                id,
                "booking_failed",
                json!({}),
                idempotency(&headers),
                req.occurred_at,
            )
            .await?;
        }
        "allow" => {
            let precedent = resolve_precedent(
                &mut tx,
                user.id,
                booking.chain_id,
                "auxiliary",
                req.precedent_id,
                req.new_rule,
            )
            .await?;
            sqlx::query("UPDATE bookings SET state='allowed',resolved_at=now(),precedent_id=$2,version=version+1 WHERE id=$1").bind(id).bind(precedent).execute(&mut *tx).await?;
            sqlx::query("UPDATE chains SET auxiliary_streak=auxiliary_streak+1,version=version+1,updated_at=now() WHERE id=$1 AND user_id=$2")
          .bind(booking.chain_id).bind(user.id).execute(&mut *tx).await?;
            insert_event(
                &mut tx,
                user.id,
                Some(booking.chain_id),
                "booking",
                id,
                "booking_allowed",
                json!({"precedentId":precedent}),
                idempotency(&headers),
                req.occurred_at,
            )
            .await?;
        }
        _ => return Err(ApiError::validation("decision 必须是 failure 或 allow")),
    }
    let result=sqlx::query_as::<_,Booking>("SELECT id,chain_id,state::text AS state,scheduled_at,expires_at,confirmation_expires_at,resolved_at,version FROM bookings WHERE id=$1").bind(id).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(result))
}

async fn start_group_run(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<(StatusCode, Json<GroupRun>)> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    let mut tx = state.pool.begin().await?;
    let chain = fetch_chain_tx(&mut tx, user.id, id).await?;
    if !matches!(chain.kind, ChainKind::Group) {
        return Err(ApiError::validation("只有任务群可以创建运行周期"));
    }
    let leaves=sqlx::query_as::<_,(Uuid,)>(
      "WITH RECURSIVE tree AS (SELECT id,kind,ARRAY[position] path FROM chains WHERE parent_id=$1 AND user_id=$2 AND archived_at IS NULL UNION ALL SELECT c.id,c.kind,t.path||c.position FROM chains c JOIN tree t ON c.parent_id=t.id WHERE c.user_id=$2 AND c.archived_at IS NULL) SELECT c.id FROM chains c JOIN tree t ON t.id=c.id WHERE t.kind='unit' ORDER BY t.path,c.id LIMIT 1001 FOR UPDATE OF c")
      .bind(id).bind(user.id).fetch_all(&mut *tx).await?;
    if leaves.is_empty() {
        return Err(ApiError::validation("空任务群不能启动"));
    }
    if leaves.len() > 1000 {
        return Err(ApiError::Conflict(
            "单次任务群运行最多包含 1000 个叶子任务".to_owned(),
        ));
    }
    let run_id = Uuid::now_v7();
    sqlx::query("INSERT INTO group_runs (id,user_id,chain_id,expires_at) VALUES ($1,$2,$3,CASE WHEN $4::integer IS NULL THEN NULL ELSE now()+make_interval(secs=>$4) END)")
      .bind(run_id).bind(user.id).bind(id).bind(chain.group_time_limit_seconds).execute(&mut *tx).await.map_err(map_unique_conflict("该任务群已有运行周期"))?;
    for (position, (chain_id,)) in leaves.into_iter().enumerate() {
        sqlx::query(
            "INSERT INTO group_run_items (id,group_run_id,chain_id,position) VALUES ($1,$2,$3,$4)",
        )
        .bind(Uuid::now_v7())
        .bind(run_id)
        .bind(chain_id)
        .bind(i32::try_from(position).unwrap_or(i32::MAX))
        .execute(&mut *tx)
        .await?;
    }
    insert_event(
        &mut tx,
        user.id,
        Some(id),
        "group_run",
        run_id,
        "group_run_started",
        json!({}),
        idempotency(&headers),
        Utc::now(),
    )
    .await?;
    if let Some(due) = chain
        .group_time_limit_seconds
        .map(|seconds| Utc::now() + chrono::Duration::seconds(i64::from(seconds)))
    {
        sqlx::query("INSERT INTO notification_jobs(id,user_id,event_key,title,subtitle,body,target_path,due_at,next_attempt_at) VALUES($1,$2,$3,'任务群时限已到',$4,'运行周期已经到达时限。\n现在打开 Lumeno 完成判定。','/chains',$5,$5) ON CONFLICT(user_id,event_key) DO NOTHING")
            .bind(Uuid::now_v7()).bind(user.id).bind(format!("group-expiry-{run_id}")).bind(&chain.name).bind(due).execute(&mut *tx).await?;
    }
    let result = fetch_group_run_tx(&mut tx, user.id, run_id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(result)))
}

async fn active_group_run(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Option<GroupRun>>> {
    let user = auth::require_user(&state.pool, &jar, None, false).await?;
    reconcile_due_states(&state.pool).await?;
    let run = sqlx::query_as::<_, GroupRun>(
        "SELECT r.id,r.chain_id,r.state::text AS state,r.started_at,r.expires_at,r.completed_at,r.version,\
         (SELECT i.chain_id FROM group_run_items i WHERE i.group_run_id=r.id AND i.completed_at IS NULL ORDER BY i.position LIMIT 1) next_chain_id \
         FROM group_runs r WHERE r.user_id=$1 AND r.chain_id=$2 AND r.state IN ('running','awaiting_judgment') LIMIT 1",
    )
    .bind(user.id)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    Ok(Json(run))
}

async fn resolve_group_judgment(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(req): Json<ResolveJudgmentRequest>,
) -> ApiResult<Json<GroupRun>> {
    let user = auth::require_user(&state.pool, &jar, Some(&headers), true).await?;
    domain::validate_offline_occurrence(Utc::now(), req.occurred_at)?;
    if previously_applied(&state.pool, user.id, idempotency(&headers), id).await? {
        let mut tx = state.pool.begin().await?;
        let result = fetch_group_run_tx(&mut tx, user.id, id).await?;
        tx.commit().await?;
        return Ok(Json(result));
    }
    let mut tx = state.pool.begin().await?;
    let run = fetch_group_run_tx(&mut tx, user.id, id).await?;
    if run.state != "awaiting_judgment" || run.version != req.version {
        return Err(ApiError::Conflict("任务群判定状态已改变".to_owned()));
    }
    match req.decision.as_str() {
        "failure" => {
            sqlx::query("UPDATE group_runs SET state='failed',completed_at=now(),version=version+1 WHERE id=$1")
                .bind(id).execute(&mut *tx).await?;
            sqlx::query("UPDATE focus_sessions SET group_run_id=NULL,group_run_item_id=NULL,version=version+1 WHERE user_id=$1 AND group_run_id=$2 AND state IN ('running','completion_ready','awaiting_judgment')")
                .bind(user.id).bind(id).execute(&mut *tx).await?;
            sqlx::query("UPDATE chains SET main_streak=0,total_failures=total_failures+1,version=version+1,updated_at=now() WHERE id=$1 AND user_id=$2")
                .bind(run.chain_id).bind(user.id).execute(&mut *tx).await?;
            insert_event(
                &mut tx,
                user.id,
                Some(run.chain_id),
                "group_run",
                id,
                "group_run_failed",
                json!({}),
                idempotency(&headers),
                req.occurred_at,
            )
            .await?;
        }
        "allow" => {
            let precedent = resolve_precedent(
                &mut tx,
                user.id,
                run.chain_id,
                "group_deadline",
                req.precedent_id,
                req.new_rule,
            )
            .await?;
            let remaining: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM group_run_items WHERE group_run_id=$1 AND completed_at IS NULL",
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            if remaining == 0 {
                sqlx::query("UPDATE group_runs SET state='succeeded',expires_at=NULL,completed_at=now(),version=version+1 WHERE id=$1")
                    .bind(id).execute(&mut *tx).await?;
                sqlx::query("UPDATE chains SET main_streak=main_streak+1,total_completions=total_completions+1,version=version+1,updated_at=now() WHERE id=$1 AND user_id=$2")
                    .bind(run.chain_id).bind(user.id).execute(&mut *tx).await?;
                insert_event(
                    &mut tx,
                    user.id,
                    Some(run.chain_id),
                    "group_run",
                    id,
                    "group_run_succeeded",
                    json!({}),
                    None,
                    req.occurred_at,
                )
                .await?;
            } else {
                sqlx::query("UPDATE group_runs SET state='running',expires_at=NULL,version=version+1 WHERE id=$1")
                    .bind(id).execute(&mut *tx).await?;
            }
            insert_event(
                &mut tx,
                user.id,
                Some(run.chain_id),
                "group_run",
                id,
                "group_deadline_allowed",
                json!({"precedentId":precedent}),
                idempotency(&headers),
                req.occurred_at,
            )
            .await?;
        }
        _ => return Err(ApiError::validation("decision 必须是 failure 或 allow")),
    }
    let result = fetch_group_run_tx(&mut tx, user.id, id).await?;
    tx.commit().await?;
    Ok(Json(result))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventQuery {
    cursor_at: Option<DateTime<Utc>>,
    cursor_id: Option<Uuid>,
    limit: Option<i64>,
}
async fn list_events(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<EventQuery>,
) -> ApiResult<Json<Vec<ProtocolEvent>>> {
    let user = auth::require_user(&state.pool, &jar, None, false).await?;
    if query.cursor_at.is_some() != query.cursor_id.is_some() {
        return Err(ApiError::validation(
            "历史游标必须同时包含 cursorAt 与 cursorId",
        ));
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let rows=sqlx::query_as::<_,ProtocolEvent>("SELECT id,chain_id,aggregate_type,aggregate_id,event_type,payload,occurred_at,recorded_at FROM protocol_events WHERE user_id=$1 AND ($2::timestamptz IS NULL OR (recorded_at,id)<($2,$3::uuid)) ORDER BY recorded_at DESC,id DESC LIMIT $4")
      .bind(user.id).bind(query.cursor_at).bind(query.cursor_id).bind(limit).fetch_all(&state.pool).await?;
    Ok(Json(rows))
}

async fn commitment_record(
    State(state): State<AppState>,
    jar: CookieJar,
) -> ApiResult<Json<CommitmentRecord>> {
    let user = auth::require_user(&state.pool, &jar, None, false).await?;
    Ok(Json(fetch_commitment_record(&state.pool, user.id).await?))
}

pub(crate) async fn fetch_commitment_record(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<CommitmentRecord, sqlx::Error> {
    let record = sqlx::query_as::<_, CommitmentRecord>(
        "SELECT \
           count(*) FILTER (WHERE event_type=ANY($2))::bigint AS completions, \
           count(*) FILTER (WHERE event_type=ANY($3))::bigint AS failures \
         FROM protocol_events WHERE user_id=$1",
    )
    .bind(user_id)
    .bind(COMMITMENT_COMPLETION_EVENTS)
    .bind(COMMITMENT_FAILURE_EVENTS)
    .fetch_one(pool)
    .await?;
    Ok(record)
}

async fn create_revision(
    pool: &PgPool,
    user_id: Uuid,
    current: Chain,
    request: UpdateChainRequest,
) -> ApiResult<Chain> {
    let mut tx = pool.begin().await?;
    lock_chain_mutations(&mut tx, user_id).await?;
    let locked_version: Option<i64> = sqlx::query_scalar(
        "SELECT version FROM chains WHERE id=$1 AND user_id=$2 AND archived_at IS NULL FOR UPDATE",
    )
    .bind(current.id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?;
    if locked_version != Some(request.version) {
        return Err(ApiError::Conflict("链已在其他设备更新".to_owned()));
    }
    validate_parent(&mut tx, user_id, request.parent_id, Some(current.id)).await?;
    let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM focus_sessions WHERE user_id=$1 AND chain_id=$2 AND state IN ('running','completion_ready','awaiting_judgment') UNION ALL SELECT 1 FROM bookings WHERE user_id=$1 AND chain_id=$2 AND state IN ('pending','awaiting_judgment') UNION ALL SELECT 1 FROM group_runs WHERE user_id=$1 AND chain_id=$2 AND state IN ('running','awaiting_judgment') UNION ALL SELECT 1 FROM group_run_items i JOIN group_runs r ON r.id=i.group_run_id WHERE r.user_id=$1 AND i.chain_id=$2 AND i.completed_at IS NULL AND r.state IN ('running','awaiting_judgment'))")
      .bind(user_id).bind(current.id).fetch_one(&mut *tx).await?;
    if active {
        return Err(ApiError::Conflict(
            "活动会话结束前不能修改核心承诺".to_owned(),
        ));
    }
    let archived=sqlx::query("UPDATE chains SET archived_at=now(),version=version+1,updated_at=now() WHERE id=$1 AND user_id=$2 AND version=$3 AND archived_at IS NULL")
      .bind(current.id).bind(user_id).bind(request.version).execute(&mut *tx).await?;
    if archived.rows_affected() == 0 {
        return Err(ApiError::Conflict("链已在其他设备更新".to_owned()));
    }
    let id = Uuid::now_v7();
    let query = format!(
        "INSERT INTO chains (id,user_id,logical_id,revision,previous_revision_id,parent_id,kind,unit_kind,name,description,position,sacred_trigger,duration_seconds,auxiliary_signal,auxiliary_delay_seconds,auxiliary_completion_trigger,group_time_limit_seconds) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17) RETURNING {CHAIN_COLUMNS}"
    );
    let row = sqlx::query_as::<_, Chain>(sqlx::AssertSqlSafe(query))
        .bind(id)
        .bind(user_id)
        .bind(current.logical_id)
        .bind(current.revision + 1)
        .bind(current.id)
        .bind(request.parent_id)
        .bind(current.kind)
        .bind(current.unit_kind)
        .bind(request.name.trim())
        .bind(request.description.trim())
        .bind(request.position)
        .bind(trim_option(request.sacred_trigger))
        .bind(request.duration_seconds)
        .bind(trim_option(request.auxiliary_signal))
        .bind(request.auxiliary_delay_seconds)
        .bind(trim_option(request.auxiliary_completion_trigger))
        .bind(request.group_time_limit_seconds)
        .fetch_one(&mut *tx)
        .await?;
    if matches!(current.kind, ChainKind::Group) {
        sqlx::query("UPDATE chains SET parent_id=$1,version=version+1,updated_at=now() WHERE user_id=$2 AND parent_id=$3 AND archived_at IS NULL")
            .bind(id)
            .bind(user_id)
            .bind(current.id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(row)
}

fn validate_chain_request(req: &CreateChainRequest) -> ApiResult<()> {
    if req.name.trim().is_empty() {
        return Err(ApiError::validation("名称不能为空"));
    }
    match req.kind {
        ChainKind::Group => {
            if req.unit_kind.is_some()
                || req.sacred_trigger.is_some()
                || req.duration_seconds.is_some()
                || req.auxiliary_signal.is_some()
                || req.auxiliary_delay_seconds.is_some()
                || req.auxiliary_completion_trigger.is_some()
            {
                return Err(ApiError::validation("任务群不能包含叶子任务承诺字段"));
            }
        }
        ChainKind::Unit => {
            if req.unit_kind.is_none()
                || trim_ref(&req.sacred_trigger).is_none()
                || trim_ref(&req.auxiliary_signal).is_none()
                || req.auxiliary_delay_seconds.is_none()
                || trim_ref(&req.auxiliary_completion_trigger).is_none()
            {
                return Err(ApiError::validation("叶子任务缺少完整 CTDP 承诺"));
            }
            if req.group_time_limit_seconds.is_some() {
                return Err(ApiError::validation("叶子任务不能设置任务群时限"));
            }
        }
    }
    validate_common_fields(
        &req.name,
        &req.description,
        req.duration_seconds,
        req.auxiliary_delay_seconds,
        req.group_time_limit_seconds,
    )?;
    validate_commitment_texts(
        &req.sacred_trigger,
        &req.auxiliary_signal,
        &req.auxiliary_completion_trigger,
    )?;
    Ok(())
}

fn validate_chain_update(current: &Chain, req: &UpdateChainRequest) -> ApiResult<()> {
    match current.kind {
        ChainKind::Group => {
            if req.sacred_trigger.is_some()
                || req.duration_seconds.is_some()
                || req.auxiliary_signal.is_some()
                || req.auxiliary_delay_seconds.is_some()
                || req.auxiliary_completion_trigger.is_some()
            {
                return Err(ApiError::validation("任务群不能包含叶子任务承诺字段"));
            }
        }
        ChainKind::Unit => {
            if trim_ref(&req.sacred_trigger).is_none()
                || trim_ref(&req.auxiliary_signal).is_none()
                || req.auxiliary_delay_seconds.is_none()
                || trim_ref(&req.auxiliary_completion_trigger).is_none()
            {
                return Err(ApiError::validation("叶子任务缺少完整 CTDP 承诺"));
            }
            if req.group_time_limit_seconds.is_some() {
                return Err(ApiError::validation("叶子任务不能设置任务群时限"));
            }
        }
    }
    validate_common_fields(
        &req.name,
        &req.description,
        req.duration_seconds,
        req.auxiliary_delay_seconds,
        req.group_time_limit_seconds,
    )?;
    validate_commitment_texts(
        &req.sacred_trigger,
        &req.auxiliary_signal,
        &req.auxiliary_completion_trigger,
    )
}

fn validate_commitment_texts(
    sacred_trigger: &Option<String>,
    auxiliary_signal: &Option<String>,
    auxiliary_completion_trigger: &Option<String>,
) -> ApiResult<()> {
    if [
        sacred_trigger,
        auxiliary_signal,
        auxiliary_completion_trigger,
    ]
    .into_iter()
    .flatten()
    .any(|value| value.trim().chars().count() > 1000)
    {
        return Err(ApiError::validation("承诺条件不能超过 1000 个字符"));
    }
    Ok(())
}

fn validate_common_fields(
    name: &str,
    description: &str,
    duration: Option<i32>,
    auxiliary_delay: Option<i32>,
    group_limit: Option<i32>,
) -> ApiResult<()> {
    if name.trim().is_empty() || name.chars().count() > 120 {
        return Err(ApiError::validation("名称长度必须为 1–120 个字符"));
    }
    if description.chars().count() > 4000 {
        return Err(ApiError::validation("描述不能超过 4000 个字符"));
    }
    if duration.is_some_and(|value| !(60..=86_400).contains(&value))
        || auxiliary_delay.is_some_and(|value| !(60..=86_400).contains(&value))
        || group_limit.is_some_and(|value| !(60..=604_800).contains(&value))
    {
        return Err(ApiError::validation("时长超出允许范围"));
    }
    Ok(())
}
async fn validate_parent(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    parent: Option<Uuid>,
    self_id: Option<Uuid>,
) -> ApiResult<()> {
    let Some(parent) = parent else { return Ok(()) };
    let kind: Option<String> = sqlx::query_scalar(
        "SELECT kind::text FROM chains WHERE id=$1 AND user_id=$2 AND archived_at IS NULL",
    )
    .bind(parent)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?;
    if kind.as_deref() != Some("group") {
        return Err(ApiError::validation("父节点必须是活动任务群"));
    }
    if let Some(self_id) = self_id {
        let cycle:bool=sqlx::query_scalar("WITH RECURSIVE descendants AS (SELECT id FROM chains WHERE parent_id=$1 AND user_id=$2 AND archived_at IS NULL UNION ALL SELECT c.id FROM chains c JOIN descendants d ON c.parent_id=d.id WHERE c.user_id=$2 AND c.archived_at IS NULL) SELECT EXISTS(SELECT 1 FROM descendants WHERE id=$3)")
       .bind(self_id).bind(user_id).bind(parent).fetch_one(&mut **tx).await?;
        if cycle || parent == self_id {
            return Err(ApiError::validation("任务树不能形成循环"));
        }
    }
    Ok(())
}

async fn lock_chain_mutations(tx: &mut Transaction<'_, Postgres>, user_id: Uuid) -> ApiResult<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 1))")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
async fn fetch_chain(pool: &PgPool, user_id: Uuid, id: Uuid) -> ApiResult<Chain> {
    let query = format!(
        "SELECT {CHAIN_COLUMNS} FROM chains WHERE id=$1 AND user_id=$2 AND archived_at IS NULL"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(query))
        .bind(id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound)
}
async fn fetch_chain_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    id: Uuid,
) -> ApiResult<Chain> {
    let query = format!(
        "SELECT {CHAIN_COLUMNS} FROM chains WHERE id=$1 AND user_id=$2 AND archived_at IS NULL FOR UPDATE"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(query))
        .bind(id)
        .bind(user_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(ApiError::NotFound)
}
async fn fetch_focus_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    id: Uuid,
) -> ApiResult<FocusSession> {
    sqlx::query_as("SELECT id,chain_id,booking_id,group_run_id,state::text AS state,started_at,completion_available_at,pending_action,pending_reason,version FROM focus_sessions WHERE id=$1 AND user_id=$2").bind(id).bind(user_id).fetch_optional(&mut **tx).await?.ok_or(ApiError::NotFound)
}
async fn fetch_focus(pool: &PgPool, user_id: Uuid, id: Uuid) -> ApiResult<FocusSession> {
    sqlx::query_as("SELECT id,chain_id,booking_id,group_run_id,state::text AS state,started_at,completion_available_at,pending_action,pending_reason,version FROM focus_sessions WHERE id=$1 AND user_id=$2")
        .bind(id).bind(user_id).fetch_optional(pool).await?.ok_or(ApiError::NotFound)
}
async fn lock_focus(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    id: Uuid,
) -> ApiResult<FocusSession> {
    sqlx::query_as("SELECT id,chain_id,booking_id,group_run_id,state::text AS state,started_at,completion_available_at,pending_action,pending_reason,version FROM focus_sessions WHERE id=$1 AND user_id=$2 FOR UPDATE").bind(id).bind(user_id).fetch_optional(&mut **tx).await?.ok_or(ApiError::NotFound)
}

async fn fetch_group_run_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    id: Uuid,
) -> ApiResult<GroupRun> {
    sqlx::query_as(
        "SELECT r.id,r.chain_id,r.state::text AS state,r.started_at,r.expires_at,r.completed_at,r.version,\
         (SELECT i.chain_id FROM group_run_items i WHERE i.group_run_id=r.id AND i.completed_at IS NULL ORDER BY i.position LIMIT 1) next_chain_id \
         FROM group_runs r WHERE r.id=$1 AND r.user_id=$2 FOR UPDATE OF r",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(ApiError::NotFound)
}

async fn succeed_focus(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    s: &FocusSession,
    key: Option<Uuid>,
    occurred: DateTime<Utc>,
) -> ApiResult<()> {
    sqlx::query("UPDATE focus_sessions SET state='succeeded',resolved_at=now(),pending_action=NULL,pending_reason=NULL,version=version+1 WHERE id=$1").bind(s.id).execute(&mut **tx).await?;
    cancel_future_focus_notification(tx, user_id, s).await?;
    sqlx::query("UPDATE chains SET main_streak=main_streak+1,total_completions=total_completions+1,version=version+1,updated_at=now() WHERE id=$1 AND user_id=$2").bind(s.chain_id).bind(user_id).execute(&mut **tx).await?;
    if let Some(run_id) = s.group_run_id {
        sqlx::query("UPDATE group_run_items SET completed_at=now() WHERE id=(SELECT group_run_item_id FROM focus_sessions WHERE id=$1) AND completed_at IS NULL").bind(s.id).execute(&mut **tx).await?;
        let remaining: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM group_run_items WHERE group_run_id=$1 AND completed_at IS NULL",
        )
        .bind(run_id)
        .fetch_one(&mut **tx)
        .await?;
        if remaining == 0 {
            sqlx::query("UPDATE group_runs SET state='awaiting_judgment',version=version+1 WHERE id=$1 AND user_id=$2 AND state='running' AND expires_at IS NOT NULL AND expires_at<=now()")
                .bind(run_id).bind(user_id).execute(&mut **tx).await?;
            let group_id:Option<Uuid>=sqlx::query_scalar("UPDATE group_runs SET state='succeeded',completed_at=now(),version=version+1 WHERE id=$1 AND user_id=$2 AND state='running' AND (expires_at IS NULL OR expires_at>now()) RETURNING chain_id").bind(run_id).bind(user_id).fetch_optional(&mut **tx).await?;
            if let Some(group_id) = group_id {
                sqlx::query("UPDATE notification_jobs SET cancelled_at=now() WHERE user_id=$1 AND event_key=$2 AND sent_at IS NULL AND failed_at IS NULL AND cancelled_at IS NULL")
                    .bind(user_id).bind(format!("group-expiry-{run_id}")).execute(&mut **tx).await?;
                sqlx::query("UPDATE chains SET main_streak=main_streak+1,total_completions=total_completions+1,version=version+1,updated_at=now() WHERE id=$1 AND user_id=$2").bind(group_id).bind(user_id).execute(&mut **tx).await?;
                insert_event(
                    tx,
                    user_id,
                    Some(group_id),
                    "group_run",
                    run_id,
                    "group_run_succeeded",
                    json!({}),
                    None,
                    occurred,
                )
                .await?;
            }
        }
    }
    insert_event(
        tx,
        user_id,
        Some(s.chain_id),
        "focus",
        s.id,
        "focus_succeeded",
        json!({}),
        key,
        occurred,
    )
    .await
}
async fn fail_focus(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    s: &FocusSession,
    key: Option<Uuid>,
    occurred: DateTime<Utc>,
) -> ApiResult<()> {
    sqlx::query(
        "UPDATE focus_sessions SET state='failed',resolved_at=now(),version=version+1 WHERE id=$1",
    )
    .bind(s.id)
    .execute(&mut **tx)
    .await?;
    cancel_future_focus_notification(tx, user_id, s).await?;
    sqlx::query("UPDATE chains SET main_streak=0,total_failures=total_failures+1,version=version+1,updated_at=now() WHERE id=$1 AND user_id=$2").bind(s.chain_id).bind(user_id).execute(&mut **tx).await?;
    insert_event(
        tx,
        user_id,
        Some(s.chain_id),
        "focus",
        s.id,
        "focus_failed",
        json!({"reason":s.pending_reason}),
        key,
        occurred,
    )
    .await
}
async fn cancel_future_focus_notification(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    session: &FocusSession,
) -> ApiResult<()> {
    if session
        .completion_available_at
        .is_some_and(|deadline| deadline > Utc::now())
    {
        sqlx::query(
            "UPDATE notification_jobs SET cancelled_at=now() WHERE user_id=$1 AND event_key=$2 \
             AND sent_at IS NULL AND failed_at IS NULL AND cancelled_at IS NULL",
        )
        .bind(user_id)
        .bind(format!("focus-completion-{}", session.id))
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}
async fn resolve_precedent(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    chain: Uuid,
    scope: &str,
    existing: Option<Uuid>,
    new_rule: Option<String>,
) -> ApiResult<Uuid> {
    if let Some(id) = existing {
        let found:Option<Uuid>=sqlx::query_scalar("SELECT id FROM precedents WHERE id=$1 AND user_id=$2 AND chain_id=$3 AND scope::text=$4").bind(id).bind(user).bind(chain).bind(scope).fetch_optional(&mut **tx).await?;
        return found.ok_or_else(|| ApiError::validation("判例不属于当前链"));
    }
    let rule = new_rule
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ApiError::validation("允许判定必须选择或创建永久判例"))?;
    if rule.chars().count() > 1000 {
        return Err(ApiError::validation("永久判例不能超过 1000 个字符"));
    }
    let id = Uuid::now_v7();
    sqlx::query_scalar("INSERT INTO precedents(id,user_id,chain_id,scope,rule) VALUES($1,$2,$3,$4::judgment_scope,$5) ON CONFLICT(chain_id,scope,rule) DO UPDATE SET rule=EXCLUDED.rule RETURNING id").bind(id).bind(user).bind(chain).bind(scope).bind(rule).fetch_one(&mut **tx).await.map_err(Into::into)
}
async fn next_group_item(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    run: Uuid,
    chain: Uuid,
) -> ApiResult<Option<Uuid>> {
    let next:Option<(Uuid,Uuid)>=sqlx::query_as("SELECT i.id,i.chain_id FROM group_run_items i JOIN group_runs r ON r.id=i.group_run_id WHERE r.id=$1 AND r.user_id=$2 AND r.state='running' AND (r.expires_at IS NULL OR r.expires_at>now()) AND i.completed_at IS NULL ORDER BY i.position LIMIT 1 FOR UPDATE OF i").bind(run).bind(user).fetch_optional(&mut **tx).await?;
    match next {
        Some((item, c)) if c == chain => Ok(Some(item)),
        Some(_) => Err(ApiError::Conflict(
            "必须按任务群顺序开始下一个单元".to_owned(),
        )),
        None => Err(ApiError::Conflict("任务群没有待完成单元".to_owned())),
    }
}

pub async fn reconcile_due_states(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("WITH due AS (SELECT id FROM bookings WHERE state='pending' AND expires_at<=now() ORDER BY expires_at LIMIT 1000 FOR UPDATE SKIP LOCKED) UPDATE bookings SET state='awaiting_judgment',confirmation_expires_at=expires_at+make_interval(mins=>$1),version=version+1 WHERE id IN(SELECT id FROM due)")
        .bind(BOOKING_CONFIRMATION_WINDOW_MINUTES)
        .execute(pool)
        .await?;
    sqlx::query(
        "WITH due AS (\
           SELECT id FROM bookings WHERE state='awaiting_judgment' AND confirmation_expires_at<=now() \
           ORDER BY confirmation_expires_at LIMIT 1000 FOR UPDATE SKIP LOCKED\
         ), failed AS (\
           UPDATE bookings b SET state='failed',resolved_at=b.confirmation_expires_at,version=b.version+1 \
           FROM due WHERE b.id=due.id \
           RETURNING b.id,b.user_id,b.chain_id,b.confirmation_expires_at\
         ), failure_counts AS (\
           SELECT user_id,chain_id,count(*)::bigint AS failures FROM failed GROUP BY user_id,chain_id\
         ), updated_chains AS (\
           UPDATE chains c SET auxiliary_streak=0,auxiliary_failures=c.auxiliary_failures+f.failures,\
             version=c.version+1,updated_at=now() FROM failure_counts f \
           WHERE c.id=f.chain_id AND c.user_id=f.user_id RETURNING c.id\
         ) INSERT INTO protocol_events(\
           id,user_id,chain_id,aggregate_type,aggregate_id,event_type,payload,occurred_at\
         ) SELECT uuidv7(),user_id,chain_id,'booking',id,'booking_confirmation_timed_out',\
           jsonb_build_object('reason','confirmation_timeout'),confirmation_expires_at FROM failed",
    )
    .execute(pool)
    .await?;
    sqlx::query("WITH due AS (SELECT id FROM focus_sessions WHERE state='running' AND completion_available_at IS NOT NULL AND completion_available_at<=now() ORDER BY completion_available_at LIMIT 1000 FOR UPDATE SKIP LOCKED) UPDATE focus_sessions SET state='completion_ready',version=version+1 WHERE id IN(SELECT id FROM due)").execute(pool).await?;
    sqlx::query("WITH due AS (SELECT id FROM group_runs WHERE state='running' AND expires_at IS NOT NULL AND expires_at<=now() ORDER BY expires_at LIMIT 1000 FOR UPDATE SKIP LOCKED) UPDATE group_runs SET state='awaiting_judgment',version=version+1 WHERE id IN(SELECT id FROM due)").execute(pool).await?;
    Ok(())
}
// Keeping the immutable ledger fields explicit at each call site makes protocol audits easier.
#[allow(clippy::too_many_arguments)]
async fn insert_event(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    chain: Option<Uuid>,
    aggregate_type: &str,
    aggregate: Uuid,
    event_type: &str,
    payload: serde_json::Value,
    key: Option<Uuid>,
    occurred: DateTime<Utc>,
) -> ApiResult<()> {
    sqlx::query("INSERT INTO protocol_events(id,user_id,chain_id,aggregate_type,aggregate_id,event_type,payload,idempotency_key,occurred_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(user_id,idempotency_key) DO NOTHING").bind(Uuid::now_v7()).bind(user).bind(chain).bind(aggregate_type).bind(aggregate).bind(event_type).bind(payload).bind(key).bind(occurred).execute(&mut **tx).await?;
    Ok(())
}
struct NotificationSpec<'a> {
    event_key: String,
    title: &'a str,
    subtitle: Option<&'a str>,
    body: &'a str,
    path: &'a str,
    due: DateTime<Utc>,
}

async fn enqueue_notification(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    notification: NotificationSpec<'_>,
) -> ApiResult<()> {
    sqlx::query("INSERT INTO notification_jobs(id,user_id,event_key,title,subtitle,body,target_path,due_at,next_attempt_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$8) ON CONFLICT(user_id,event_key) DO NOTHING").bind(Uuid::now_v7()).bind(user).bind(notification.event_key).bind(notification.title).bind(notification.subtitle).bind(notification.body).bind(notification.path).bind(notification.due).execute(&mut **tx).await?;
    Ok(())
}
fn idempotency(headers: &HeaderMap) -> Option<Uuid> {
    headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| Uuid::parse_str(v).ok())
}

async fn previously_applied(
    pool: &PgPool,
    user: Uuid,
    key: Option<Uuid>,
    aggregate: Uuid,
) -> ApiResult<bool> {
    let Some(key) = key else { return Ok(false) };
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT aggregate_id FROM protocol_events WHERE user_id=$1 AND idempotency_key=$2",
    )
    .bind(user)
    .bind(key)
    .fetch_optional(pool)
    .await?;
    match existing {
        Some(existing) if existing == aggregate => Ok(true),
        Some(_) => Err(ApiError::Conflict("幂等键已用于其他操作".to_owned())),
        None => Ok(false),
    }
}
fn trim_option(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}
fn trim_ref(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty())
}
fn map_unique_conflict(message: &'static str) -> impl FnOnce(sqlx::Error) -> ApiError {
    move |error| match &error {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            ApiError::Conflict(message.to_owned())
        }
        _ => ApiError::Database(error),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{COMMITMENT_COMPLETION_EVENTS, COMMITMENT_FAILURE_EVENTS};

    #[test]
    fn commitment_record_event_sets_match_counting_rules_and_are_disjoint() {
        assert_eq!(
            COMMITMENT_COMPLETION_EVENTS,
            ["focus_succeeded", "group_run_succeeded"]
        );
        assert_eq!(
            COMMITMENT_FAILURE_EVENTS,
            [
                "focus_failed",
                "group_run_failed",
                "booking_failed",
                "booking_confirmation_timed_out",
            ]
        );
        let completions = COMMITMENT_COMPLETION_EVENTS
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        assert!(
            COMMITMENT_FAILURE_EVENTS
                .iter()
                .all(|event| !completions.contains(event)),
            "a terminal event cannot count as both completion and failure"
        );
    }
}
