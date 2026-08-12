use sqlx::PgPool;
use uuid::Uuid;

async fn seed_user(pool: &PgPool) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO app_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'https://issuer.example','subject','Tester')",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(id)
}

async fn seed_unit(pool: &PgPool, user: Uuid, name: &str) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO chains(id,user_id,logical_id,revision,kind,unit_kind,name,sacred_trigger,auxiliary_signal,auxiliary_delay_seconds,auxiliary_completion_trigger) \
         VALUES($1,$2,$1,1,'unit','basic',$3,'坐下','预约',900,'按时开始')",
    )
    .bind(id)
    .bind(user)
    .bind(name)
    .execute(pool)
    .await?;
    Ok(id)
}

#[sqlx::test(migrations = "./migrations")]
async fn database_allows_only_one_active_focus_per_user(pool: PgPool) -> sqlx::Result<()> {
    let user = seed_user(&pool).await?;
    let first_chain = seed_unit(&pool, user, "A").await?;
    let second_chain = seed_unit(&pool, user, "B").await?;
    sqlx::query("INSERT INTO focus_sessions(id,user_id,chain_id) VALUES($1,$2,$3)")
        .bind(Uuid::now_v7())
        .bind(user)
        .bind(first_chain)
        .execute(&pool)
        .await?;
    let error = sqlx::query("INSERT INTO focus_sessions(id,user_id,chain_id) VALUES($1,$2,$3)")
        .bind(Uuid::now_v7())
        .bind(user)
        .bind(second_chain)
        .execute(&pool)
        .await
        .expect_err("a second active focus must violate the partial unique index");
    assert!(
        error
            .as_database_error()
            .is_some_and(|db| db.is_unique_violation())
    );
    Ok(())
}

#[sqlx::test(migrations = "./migrations")]
async fn rsip_activation_is_limited_to_one_per_local_date(pool: PgPool) -> sqlx::Result<()> {
    let user = seed_user(&pool).await?;
    let insert = "INSERT INTO rsip_nodes(id,user_id,state,title,trigger_condition,required_action,activated_on,activated_at) \
                  VALUES($1,$2,'active',$3,'signal','act',DATE '2026-08-10',now())";
    sqlx::query(insert)
        .bind(Uuid::now_v7())
        .bind(user)
        .bind("first")
        .execute(&pool)
        .await?;
    let error = sqlx::query(insert)
        .bind(Uuid::now_v7())
        .bind(user)
        .bind("second")
        .execute(&pool)
        .await
        .expect_err("a second activation on the same date must fail");
    assert!(
        error
            .as_database_error()
            .is_some_and(|db| db.is_unique_violation())
    );
    Ok(())
}

#[sqlx::test(migrations = "./migrations")]
async fn one_negative_state_accepts_multiple_traceback_chains(pool: PgPool) -> sqlx::Result<()> {
    let user = seed_user(&pool).await?;
    let negative_state = Uuid::now_v7();
    sqlx::query("INSERT INTO negative_states(id,user_id,title) VALUES($1,$2,'observable state')")
        .bind(negative_state)
        .bind(user)
        .execute(&pool)
        .await?;

    for traceback_id in [Uuid::now_v7(), Uuid::now_v7()] {
        sqlx::query(
            "INSERT INTO traceback_steps(id,user_id,negative_state_id,traceback_id,position,decision) \
             VALUES($1,$2,$3,$4,0,'a reversible decision')",
        )
        .bind(Uuid::now_v7())
        .bind(user)
        .bind(negative_state)
        .bind(traceback_id)
        .execute(&pool)
        .await?;
    }

    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM traceback_steps WHERE negative_state_id=$1")
            .bind(negative_state)
            .fetch_one(&pool)
            .await?;
    assert_eq!(count, 2);
    Ok(())
}

#[sqlx::test(migrations = "./migrations")]
async fn booking_expiry_opens_a_five_minute_confirmation_window(pool: PgPool) -> sqlx::Result<()> {
    let user = seed_user(&pool).await?;
    let chain = seed_unit(&pool, user, "预约确认窗口").await?;
    let booking = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO bookings(id,user_id,chain_id,expires_at) VALUES($1,$2,$3,now()-interval '1 second')",
    )
    .bind(booking)
    .bind(user)
    .bind(chain)
    .execute(&pool)
    .await?;

    crate::ctdp::reconcile_due_states(&pool).await?;

    let (state, window_seconds): (String, Option<f64>) = sqlx::query_as(
        "SELECT state::text,EXTRACT(EPOCH FROM (confirmation_expires_at-expires_at))::double precision FROM bookings WHERE id=$1",
    )
    .bind(booking)
    .fetch_one(&pool)
    .await?;
    assert_eq!(state, "awaiting_judgment");
    assert_eq!(window_seconds, Some(300.0));
    Ok(())
}

#[sqlx::test(migrations = "./migrations")]
async fn pending_booking_keeps_a_ninety_second_start_window(pool: PgPool) -> sqlx::Result<()> {
    let user = seed_user(&pool).await?;
    let chain = seed_unit(&pool, user, "预约启动窗口").await?;
    let booking = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO bookings(id,user_id,chain_id,scheduled_at,expires_at) \
         VALUES($1,$2,$3,now(),now()+interval '90 seconds')",
    )
    .bind(booking)
    .bind(user)
    .bind(chain)
    .execute(&pool)
    .await?;

    crate::ctdp::reconcile_due_states(&pool).await?;

    let (state, window_seconds): (String, f64) = sqlx::query_as(
        "SELECT state::text,EXTRACT(EPOCH FROM (expires_at-scheduled_at))::double precision \
         FROM bookings WHERE id=$1",
    )
    .bind(booking)
    .fetch_one(&pool)
    .await?;
    assert_eq!(state, "pending");
    assert_eq!(window_seconds, 90.0);
    Ok(())
}

#[sqlx::test(migrations = "./migrations")]
async fn unconfirmed_booking_is_failed_once_after_the_confirmation_window(
    pool: PgPool,
) -> sqlx::Result<()> {
    let user = seed_user(&pool).await?;
    let chain = seed_unit(&pool, user, "预约自动失败").await?;
    sqlx::query("UPDATE chains SET auxiliary_streak=4 WHERE id=$1")
        .bind(chain)
        .execute(&pool)
        .await?;
    let booking = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO bookings(id,user_id,chain_id,expires_at) VALUES($1,$2,$3,now()-interval '6 minutes')",
    )
    .bind(booking)
    .bind(user)
    .bind(chain)
    .execute(&pool)
    .await?;

    crate::ctdp::reconcile_due_states(&pool).await?;
    crate::ctdp::reconcile_due_states(&pool).await?;

    let (state, resolved_at): (String, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT state::text,resolved_at FROM bookings WHERE id=$1")
            .bind(booking)
            .fetch_one(&pool)
            .await?;
    let (streak, failures): (i32, i64) =
        sqlx::query_as("SELECT auxiliary_streak,auxiliary_failures FROM chains WHERE id=$1")
            .bind(chain)
            .fetch_one(&pool)
            .await?;
    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM protocol_events WHERE aggregate_id=$1 AND event_type='booking_confirmation_timed_out'",
    )
    .bind(booking)
    .fetch_one(&pool)
    .await?;

    assert_eq!(state, "failed");
    assert!(resolved_at.is_some());
    assert_eq!(streak, 0);
    assert_eq!(failures, 1);
    assert_eq!(events, 1);
    Ok(())
}

#[sqlx::test(migrations = "./migrations")]
async fn commitment_record_uses_distinct_completion_and_failure_rules_across_archived_revisions(
    pool: PgPool,
) -> sqlx::Result<()> {
    let user = seed_user(&pool).await?;
    let other_user = seed_user(&pool).await?;
    let chain = seed_unit(&pool, user, "历史契约").await?;
    sqlx::query("UPDATE chains SET archived_at=now() WHERE id=$1")
        .bind(chain)
        .execute(&pool)
        .await?;

    let terminal_events = [
        "focus_succeeded",
        "group_run_succeeded",
        "booking_fulfilled",
        "booking_allowed",
        "focus_failed",
        "group_run_failed",
        "booking_failed",
        "booking_confirmation_timed_out",
        "focus_started",
    ];
    for event_type in terminal_events {
        let aggregate = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO protocol_events(id,user_id,chain_id,aggregate_type,aggregate_id,event_type,occurred_at) \
             VALUES($1,$2,$3,'test',$4,$5,now())",
        )
        .bind(Uuid::now_v7())
        .bind(user)
        .bind(chain)
        .bind(aggregate)
        .bind(event_type)
        .execute(&pool)
        .await?;
    }
    sqlx::query(
        "INSERT INTO protocol_events(id,user_id,aggregate_type,aggregate_id,event_type,occurred_at) \
         VALUES($1,$2,'test',$3,'focus_failed',now())",
    )
    .bind(Uuid::now_v7())
    .bind(other_user)
    .bind(Uuid::now_v7())
    .execute(&pool)
    .await?;

    let record = crate::ctdp::fetch_commitment_record(&pool, user).await?;
    assert_eq!(record.completions, 2);
    assert_eq!(record.failures, 4);
    Ok(())
}

#[sqlx::test(migrations = "./migrations")]
async fn archived_chains_are_user_scoped_and_ordered_by_archive_time(
    pool: PgPool,
) -> sqlx::Result<()> {
    let user = seed_user(&pool).await?;
    let other_user = seed_user(&pool).await?;
    let older = seed_unit(&pool, user, "较早归档").await?;
    let newer = seed_unit(&pool, user, "最近归档").await?;
    let active = seed_unit(&pool, user, "仍在使用").await?;
    let other = seed_unit(&pool, other_user, "其他用户归档").await?;

    sqlx::query(
        "UPDATE chains SET archived_at=CASE id \
           WHEN $1 THEN now()-interval '1 day' \
           WHEN $2 THEN now() \
           WHEN $3 THEN now()+interval '1 day' END \
         WHERE id=ANY($4)",
    )
    .bind(older)
    .bind(newer)
    .bind(other)
    .bind([older, newer, other])
    .execute(&pool)
    .await?;

    let archived = crate::ctdp::fetch_archived_chains(&pool, user).await?;
    assert_eq!(
        archived.iter().map(|chain| chain.id).collect::<Vec<_>>(),
        [newer, older]
    );
    assert!(!archived.iter().any(|chain| chain.id == active));
    Ok(())
}

#[sqlx::test(migrations = "./migrations")]
async fn rsip_failure_archives_only_the_selected_subtree(pool: PgPool) -> sqlx::Result<()> {
    let user = seed_user(&pool).await?;
    let root = Uuid::now_v7();
    let child = Uuid::now_v7();
    let draft_child = Uuid::now_v7();
    let other = Uuid::now_v7();
    for (id, parent, title) in [
        (root, None, "root"),
        (child, Some(root), "child"),
        (other, None, "other"),
    ] {
        sqlx::query("INSERT INTO rsip_nodes(id,user_id,state,title,trigger_condition,required_action,activated_on,activated_at,parent_id) VALUES($1,$2,'active',$3,'signal','act',$4,now(),$5)")
            .bind(id).bind(user).bind(title).bind(if id == other { chrono::NaiveDate::from_ymd_opt(2026,8,9).unwrap() } else if id == child { chrono::NaiveDate::from_ymd_opt(2026,8,11).unwrap() } else { chrono::NaiveDate::from_ymd_opt(2026,8,10).unwrap() }).bind(parent).execute(&pool).await?;
    }
    sqlx::query("INSERT INTO rsip_nodes(id,user_id,state,title,trigger_condition,required_action,parent_id) VALUES($1,$2,'draft','draft child','signal','act',$3)")
        .bind(draft_child).bind(user).bind(root).execute(&pool).await?;
    sqlx::query("WITH RECURSIVE subtree AS (SELECT id FROM rsip_nodes WHERE id=$1 AND user_id=$2 AND state='active' UNION ALL SELECT n.id FROM rsip_nodes n JOIN subtree s ON n.parent_id=s.id WHERE n.user_id=$2 AND n.state<>'archived') UPDATE rsip_nodes SET state='archived',archived_at=now() WHERE id IN(SELECT id FROM subtree)")
        .bind(root).bind(user).execute(&pool).await?;
    let states =
        sqlx::query_as::<_, (Uuid, String)>("SELECT id,state::text FROM rsip_nodes ORDER BY title")
            .fetch_all(&pool)
            .await?;
    assert_eq!(
        states.iter().find(|row| row.0 == root).unwrap().1,
        "archived"
    );
    assert_eq!(
        states.iter().find(|row| row.0 == child).unwrap().1,
        "archived"
    );
    assert_eq!(
        states.iter().find(|row| row.0 == draft_child).unwrap().1,
        "archived"
    );
    assert_eq!(
        states.iter().find(|row| row.0 == other).unwrap().1,
        "active"
    );
    Ok(())
}
