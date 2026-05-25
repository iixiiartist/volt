use crate::models::ToolResult;
use chrono::Utc;
use chrono_tz::Tz;
use std::time::Instant;

const SAMPLE_TZS: &[&str] = &[
    "UTC",
    "US/Eastern",
    "US/Central",
    "US/Mountain",
    "US/Pacific",
    "America/New_York",
    "America/Chicago",
    "America/Denver",
    "America/Los_Angeles",
    "Europe/London",
    "Europe/Paris",
    "Europe/Berlin",
    "Europe/Moscow",
    "Asia/Tokyo",
    "Asia/Shanghai",
    "Asia/Kolkata",
    "Asia/Dubai",
    "Australia/Sydney",
    "Pacific/Auckland",
    "Africa/Cairo",
    "America/Sao_Paulo",
];

pub async fn get_current_time(timezone: &str) -> ToolResult {
    let started = Instant::now();
    let tz: Tz = match timezone.parse() {
        Ok(t) => t,
        Err(_) => {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unknown timezone '{}'. Common timezones: {:?}",
                    timezone, SAMPLE_TZS
                )),
                duration_ms: started.elapsed().as_millis(),
            };
        }
    };
    let now = Utc::now().with_timezone(&tz);
    ToolResult {
        success: true,
        output: format!("{}", now.format("%Y-%m-%d %H:%M:%S %Z")),
        error: None,
        duration_ms: started.elapsed().as_millis(),
    }
}

pub async fn convert_time(timezone: &str, timezone_to: &str) -> ToolResult {
    let started = Instant::now();
    let from: Tz = match timezone.parse() {
        Ok(t) => t,
        Err(_) => {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unknown timezone '{}'. Common timezones: {:?}",
                    timezone, SAMPLE_TZS
                )),
                duration_ms: started.elapsed().as_millis(),
            };
        }
    };
    let to: Tz = match timezone_to.parse() {
        Ok(t) => t,
        Err(_) => {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unknown timezone '{}'. Common timezones: {:?}",
                    timezone_to, SAMPLE_TZS
                )),
                duration_ms: started.elapsed().as_millis(),
            };
        }
    };
    let now = Utc::now().with_timezone(&from);
    let converted = now.with_timezone(&to);
    ToolResult {
        success: true,
        output: format!(
            "{} → {}",
            now.format("%Y-%m-%d %H:%M:%S %Z"),
            converted.format("%Y-%m-%d %H:%M:%S %Z")
        ),
        error: None,
        duration_ms: started.elapsed().as_millis(),
    }
}
