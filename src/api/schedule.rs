//! 自動で回す処理の時刻の決め方 (しない・前回から N 時間たったら・毎日 HH:MM を過ぎたら)。

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use utoipa::ToSchema;

use crate::error::AppError;

/// 自動で回す処理の決め方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "camelCase")]
#[sqlx(rename_all = "snake_case")]
pub(super) enum AutoMode {
    /// 自動では更新しない。
    Off,
    /// 前回から `hours` 時間たったら。
    Interval,
    /// 毎日 `time` を過ぎたら。
    Daily,
}

/// 自動で回す処理の時刻の設定。グループの AI のリンクの自動の更新 (→ docs/ai.md「自動の更新」) と、
/// ライブラリの自動の再スキャン (→ docs/archive.md「スキャン」) で同じ形を使う。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct AutoRefresh {
    pub(super) mode: AutoMode,
    /// `interval` のときの時間 (1 以上)。
    pub(super) hours: Option<u32>,
    /// `daily` のときの時刻。WebLAV を動かす PC の現地時刻の `HH:MM`。
    pub(super) time: Option<String>,
}

impl AutoRefresh {
    pub(super) const OFF: Self = Self {
        mode: AutoMode::Off,
        hours: None,
        time: None,
    };

    /// DB の列から組み立てる。時間は保存の型 (`INTEGER`) から、範囲の外なら無いものとして読む。
    pub(super) fn from_columns(mode: AutoMode, hours: Option<i64>, time: Option<String>) -> Self {
        Self {
            mode,
            hours: hours.and_then(|hours| u32::try_from(hours).ok()),
            time,
        }
    }
}

/// 現地時刻の `time` (`HH:MM`) が最後に来た時刻 (UNIX 秒)。今日のその時刻がまだなら、昨日のその時刻。
pub(super) async fn last_daily_mark(pool: &SqlitePool, time: &str) -> Result<i64, AppError> {
    Ok(sqlx::query_scalar!(
        r#"SELECT CASE
               WHEN unixepoch(date('now', 'localtime') || ' ' || ?1, 'utc') <= unixepoch()
               THEN unixepoch(date('now', 'localtime') || ' ' || ?1, 'utc')
               ELSE unixepoch(date('now', 'localtime', '-1 day') || ' ' || ?1, 'utc')
           END as "mark!: i64""#,
        time
    )
    .fetch_one(pool)
    .await?)
}

/// 自動の更新の時間が来ているか。`daily_mark` は `daily` のときの、決めた時刻が最後に来た時刻。
pub(super) fn is_due(
    auto: &AutoRefresh,
    refreshed_at: Option<i64>,
    now: i64,
    daily_mark: Option<i64>,
) -> bool {
    let Some(refreshed_at) = refreshed_at else {
        return auto.mode != AutoMode::Off;
    };
    match auto.mode {
        AutoMode::Off => false,
        AutoMode::Interval => auto
            .hours
            .is_some_and(|hours| now - refreshed_at >= i64::from(hours) * 3600),
        AutoMode::Daily => daily_mark.is_some_and(|mark| refreshed_at < mark),
    }
}

/// 自動の更新の設定を確かめ、使わない項目を落とす。
pub(super) fn parse_auto_refresh(auto: AutoRefresh) -> Result<AutoRefresh, AppError> {
    match auto.mode {
        AutoMode::Off => Ok(AutoRefresh::OFF),
        AutoMode::Interval => match auto.hours {
            Some(hours) if hours >= 1 => Ok(AutoRefresh {
                mode: AutoMode::Interval,
                hours: Some(hours),
                time: None,
            }),
            _ => Err(AppError::Validation(
                "hours of the auto refresh must be 1 or more".to_string(),
            )),
        },
        AutoMode::Daily => match auto.time.as_deref().and_then(parse_time) {
            Some(time) => Ok(AutoRefresh {
                mode: AutoMode::Daily,
                hours: None,
                time: Some(time),
            }),
            None => Err(AppError::Validation(
                "time of the auto refresh must be HH:MM".to_string(),
            )),
        },
    }
}

/// `HH:MM` (00:00〜23:59) を読む。`7:00` のような1桁の時も受け、2桁に直す。
fn parse_time(text: &str) -> Option<String> {
    let (hour, minute) = text.trim().split_once(':')?;
    let digits = |part: &str, len: std::ops::RangeInclusive<usize>| {
        (len.contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| part.parse::<u32>().ok())
            .flatten()
    };
    let hour = digits(hour, 1..=2).filter(|hour| *hour < 24)?;
    let minute = digits(minute, 2..=2).filter(|minute| *minute < 60)?;
    Some(format!("{hour:02}:{minute:02}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto(mode: AutoMode, hours: Option<u32>, time: Option<&str>) -> AutoRefresh {
        AutoRefresh {
            mode,
            hours,
            time: time.map(str::to_string),
        }
    }

    #[test]
    fn parse_auto_refresh_keeps_only_the_value_of_the_chosen_mode() {
        let interval = parse_auto_refresh(auto(AutoMode::Interval, Some(6), Some("07:00")))
            .expect("読めるはず");
        assert_eq!(interval, auto(AutoMode::Interval, Some(6), None));
        let daily =
            parse_auto_refresh(auto(AutoMode::Daily, Some(6), Some("7:05"))).expect("読めるはず");
        assert_eq!(daily, auto(AutoMode::Daily, None, Some("07:05")));
        let off =
            parse_auto_refresh(auto(AutoMode::Off, Some(6), Some("07:00"))).expect("読めるはず");
        assert_eq!(off, AutoRefresh::OFF);
    }

    #[test]
    fn parse_auto_refresh_rejects_a_missing_or_zero_interval_and_a_bad_time() {
        assert!(parse_auto_refresh(auto(AutoMode::Interval, None, None)).is_err());
        assert!(parse_auto_refresh(auto(AutoMode::Interval, Some(0), None)).is_err());
        assert!(parse_auto_refresh(auto(AutoMode::Daily, None, None)).is_err());
        for time in ["24:00", "07:60", "7", "07:0", "０7:00", "07:00:00", "aa:bb"] {
            assert!(
                parse_auto_refresh(auto(AutoMode::Daily, None, Some(time))).is_err(),
                "{time}"
            );
        }
    }

    #[test]
    fn is_due_starts_at_once_when_nothing_was_collected_yet() {
        assert!(is_due(
            &auto(AutoMode::Interval, Some(6), None),
            None,
            0,
            None
        ));
        assert!(!is_due(&AutoRefresh::OFF, None, 0, None));
    }

    #[test]
    fn is_due_with_an_interval_waits_for_the_hours_from_the_last_refresh() {
        let interval = auto(AutoMode::Interval, Some(6), None);
        assert!(!is_due(&interval, Some(1_000), 1_000 + 6 * 3600 - 1, None));
        assert!(is_due(&interval, Some(1_000), 1_000 + 6 * 3600, None));
    }

    #[test]
    fn is_due_daily_waits_until_the_time_passes_after_the_last_refresh() {
        let daily = auto(AutoMode::Daily, None, Some("07:00"));
        // 7 時 (mark) より前に集めていれば、7 時を過ぎた今は時間が来ている。後に集めていれば、まだ。
        assert!(is_due(&daily, Some(900), 2_000, Some(1_000)));
        assert!(!is_due(&daily, Some(1_100), 2_000, Some(1_000)));
    }
}
