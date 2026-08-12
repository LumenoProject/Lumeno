use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::Tz;

use crate::error::{ApiError, ApiResult};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusResolution {
    Fail,
    Succeed,
    Continue,
}

pub fn focus_resolution(
    decision: &str,
    pending_action: Option<&str>,
) -> ApiResult<FocusResolution> {
    match (decision, pending_action) {
        ("failure", _) => Ok(FocusResolution::Fail),
        ("allow", Some("early_end")) => Ok(FocusResolution::Succeed),
        ("allow", Some("interrupt")) => Ok(FocusResolution::Continue),
        ("allow", _) => Err(ApiError::Conflict("判定缺少有效的待决动作".to_owned())),
        _ => Err(ApiError::validation("decision 必须是 failure 或 allow")),
    }
}

pub fn booking_is_timely(
    scheduled_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    occurred_at: DateTime<Utc>,
) -> bool {
    occurred_at >= scheduled_at && occurred_at <= expires_at
}

pub fn booking_confirmation_is_open(
    confirmation_expires_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    confirmation_expires_at.is_some_and(|deadline| now < deadline)
}

pub fn validate_offline_occurrence(
    now: DateTime<Utc>,
    occurred_at: DateTime<Utc>,
) -> ApiResult<()> {
    if occurred_at > now + Duration::minutes(5) || occurred_at < now - Duration::days(7) {
        Err(ApiError::validation("离线专注开始时间超出允许同步窗口"))
    } else {
        Ok(())
    }
}

pub fn activation_date(now: DateTime<Utc>, timezone: Tz) -> NaiveDate {
    now.with_timezone(&timezone).date_naive()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone as _;

    use super::*;

    #[test]
    fn allowed_early_end_is_success_but_allowed_interrupt_continues() {
        assert_eq!(
            focus_resolution("allow", Some("early_end")).unwrap(),
            FocusResolution::Succeed
        );
        assert_eq!(
            focus_resolution("allow", Some("interrupt")).unwrap(),
            FocusResolution::Continue
        );
        assert_eq!(
            focus_resolution("failure", Some("early_end")).unwrap(),
            FocusResolution::Fail
        );
    }

    #[test]
    fn booking_requires_start_inside_the_ninety_second_window() {
        let scheduled = Utc.with_ymd_and_hms(2026, 8, 10, 8, 15, 0).unwrap();
        let expires = scheduled + Duration::seconds(90);
        assert!(booking_is_timely(scheduled, expires, scheduled));
        assert!(booking_is_timely(scheduled, expires, expires));
        assert!(!booking_is_timely(
            scheduled,
            expires,
            expires + Duration::milliseconds(1)
        ));
        assert!(!booking_is_timely(
            scheduled,
            expires,
            scheduled - Duration::milliseconds(1)
        ));
    }

    #[test]
    fn booking_confirmation_closes_at_its_deadline() {
        let deadline = Utc.with_ymd_and_hms(2026, 8, 10, 8, 5, 0).unwrap();
        assert!(booking_confirmation_is_open(
            Some(deadline),
            deadline - Duration::milliseconds(1)
        ));
        assert!(!booking_confirmation_is_open(Some(deadline), deadline));
        assert!(!booking_confirmation_is_open(None, deadline));
    }

    #[test]
    fn offline_replay_window_is_bounded() {
        let now = Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap();
        assert!(validate_offline_occurrence(now, now - Duration::days(7)).is_ok());
        assert!(validate_offline_occurrence(now, now + Duration::minutes(5)).is_ok());
        assert!(
            validate_offline_occurrence(now, now - Duration::days(7) - Duration::seconds(1))
                .is_err()
        );
        assert!(
            validate_offline_occurrence(now, now + Duration::minutes(5) + Duration::seconds(1))
                .is_err()
        );
    }

    #[test]
    fn activation_day_uses_the_users_timezone() {
        let now = Utc.with_ymd_and_hms(2026, 8, 10, 23, 30, 0).unwrap();
        assert_eq!(
            activation_date(now, chrono_tz::Asia::Shanghai).to_string(),
            "2026-08-11"
        );
        assert_eq!(
            activation_date(now, chrono_tz::America::Los_Angeles).to_string(),
            "2026-08-10"
        );
    }
}
