use anyhow::{Context as _, anyhow};
use chrono::{DateTime, FixedOffset, LocalResult, NaiveDateTime, TimeZone, Utc};

pub const MILLISECONDS: &str = "milliseconds";
pub const SECONDS: &str = "seconds";
pub const DAYS: &str = "days";
const FORMAT: &str = "%Y-%m-%d %H:%M:%S";

fn parse_timezone(name: &str) -> anyhow::Result<FixedOffset> {
    let offset = name
        .strip_prefix("UTC")
        .ok_or_else(|| anyhow!("时区格式应为 UTC、UTC+8 或 UTC-8"))?;
    let hours = if offset.is_empty() {
        0
    } else {
        offset
            .parse::<i32>()
            .map_err(|_| anyhow!("UTC 时区偏移必须是 -12 到 +12 的整数"))?
    };
    anyhow::ensure!(
        (-12..=12).contains(&hours),
        "UTC 时区偏移必须在 -12 到 +12 之间"
    );
    Ok(FixedOffset::east_opt(hours * 3_600).expect("validated UTC offset"))
}

pub fn format_now(timezone: &str) -> anyhow::Result<String> {
    Ok(Utc::now()
        .with_timezone(&parse_timezone(timezone)?)
        .format(FORMAT)
        .to_string())
}

fn resolve_local<T: TimeZone>(naive: NaiveDateTime, timezone: &T) -> anyhow::Result<DateTime<T>> {
    match timezone.from_local_datetime(&naive) {
        LocalResult::Single(value) => Ok(value),
        LocalResult::Ambiguous(_, _) => Err(anyhow!("该本地时间因夏令时切换存在歧义")),
        LocalResult::None => Err(anyhow!("该本地时间在所选时区中不存在")),
    }
}

fn datetime_timestamp(value: &str, timezone: &str) -> anyhow::Result<(i64, i64)> {
    let naive = NaiveDateTime::parse_from_str(value.trim(), FORMAT)
        .context("时间格式应为 YYYY-MM-DD HH:mm:ss")?;
    let datetime = resolve_local(naive, &parse_timezone(timezone)?)?;
    Ok((datetime.timestamp_millis(), datetime.timestamp()))
}

pub fn timestamp_values(value: &str, timezone: &str) -> anyhow::Result<(i64, i64, i64)> {
    let (milliseconds, seconds) = datetime_timestamp(value, timezone)?;
    let days = seconds.div_euclid(86_400);
    Ok((milliseconds, seconds, days))
}

pub fn from_timestamp(value: &str, unit: &str, timezone: &str) -> anyhow::Result<String> {
    let raw = value.trim().parse::<i64>().context("时间戳必须是整数")?;
    let seconds = match unit {
        MILLISECONDS => raw.div_euclid(1_000),
        SECONDS => raw,
        DAYS => raw
            .checked_mul(86_400)
            .ok_or_else(|| anyhow!("天时间戳超出支持范围"))?,
        _ => return Err(anyhow!("未知时间戳单位")),
    };
    let nanos = if unit == MILLISECONDS {
        raw.rem_euclid(1_000) as u32 * 1_000_000
    } else {
        0
    };
    let datetime = DateTime::<Utc>::from_timestamp(seconds, nanos)
        .ok_or_else(|| anyhow!("时间戳超出支持范围"))?;
    Ok(datetime
        .with_timezone(&parse_timezone(timezone)?)
        .format(FORMAT)
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_epoch_in_utc_plus_eight() {
        let values = timestamp_values("1970-01-01 08:00:00", "UTC+8").unwrap();
        assert_eq!(values, (0, 0, 0));
        assert_eq!(
            from_timestamp("0", SECONDS, "UTC+8").unwrap(),
            "1970-01-01 08:00:00"
        );
    }

    #[test]
    fn preserves_negative_milliseconds() {
        assert_eq!(
            from_timestamp("-1", MILLISECONDS, "UTC").unwrap(),
            "1969-12-31 23:59:59"
        );
    }

    #[test]
    fn supports_fixed_utc_hour_offsets() {
        assert_eq!(
            timestamp_values("1970-01-01 12:00:00", "UTC+12").unwrap(),
            (0, 0, 0)
        );
        assert_eq!(
            from_timestamp("0", SECONDS, "UTC-12").unwrap(),
            "1969-12-31 12:00:00"
        );
        assert!(format_now("UTC+13").is_err());
    }
}
