//! Stats cache scanner for ~/.claude/stats-cache.json
//!
//! Parses the pre-computed stats cache that Claude Code maintains with
//! daily activity, token usage by model, session counts, and hourly distribution.

use color_eyre::eyre;
use std::path::PathBuf;

/// Parsed stats cache from Claude Code
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used by UI renderer and future features
pub struct StatsCache {
    pub last_computed_date: String,
    pub daily_activity: Vec<DailyActivity>,
    pub model_usage: Vec<ModelUsage>,
    pub total_sessions: u64,
    pub total_messages: u64,
    pub longest_session: Option<LongestSession>,
    pub first_session_date: String,
    pub hour_counts: [u32; 24],
}

#[derive(Debug, Clone)]
pub struct DailyActivity {
    pub date: String,
    pub messages: u64,
    pub sessions: u64,
    pub tool_calls: u64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields available for extended UI views
pub struct ModelUsage {
    pub model: String,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields available for extended UI views
pub struct LongestSession {
    pub session_id: String,
    pub duration_ms: u64,
    pub message_count: u64,
    pub timestamp: Option<String>,
}

pub struct StatsDiscovery;

impl StatsDiscovery {
    /// Read and parse ~/.claude/stats-cache.json
    pub fn scan_stats() -> eyre::Result<StatsCache> {
        let path = Self::stats_path()?;
        let content = std::fs::read_to_string(&path)?;
        let json: serde_json::Value = serde_json::from_str(&content)?;

        let last_computed_date = json
            .get("lastComputedDate")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let total_sessions = json
            .get("totalSessions")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let total_messages = json
            .get("totalMessages")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let first_session_date = json
            .get("firstSessionDate")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Parse daily activity array
        // Actual fields: date, messageCount, sessionCount, toolCallCount
        let daily_activity = json
            .get("dailyActivity")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        Some(DailyActivity {
                            date: item.get("date")?.as_str()?.to_string(),
                            messages: item
                                .get("messageCount")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0),
                            sessions: item
                                .get("sessionCount")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0),
                            tool_calls: item
                                .get("toolCallCount")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Parse model usage — an OBJECT keyed by model name, not an array
        // Each value: {inputTokens, outputTokens, cacheReadInputTokens, cacheCreationInputTokens, ...}
        let model_usage = json
            .get("modelUsage")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(model_name, item)| ModelUsage {
                        model: model_name.clone(),
                        input: item
                            .get("inputTokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                        output: item
                            .get("outputTokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                        cache_read: item
                            .get("cacheReadInputTokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                        cache_creation: item
                            .get("cacheCreationInputTokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Parse longest session
        // Actual fields: sessionId, duration (not durationMs), messageCount, timestamp
        let longest_session = json.get("longestSession").and_then(|ls| {
            Some(LongestSession {
                session_id: ls.get("sessionId")?.as_str()?.to_string(),
                duration_ms: ls.get("duration").and_then(|v| v.as_u64()).unwrap_or(0),
                message_count: ls
                    .get("messageCount")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                timestamp: ls
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            })
        });

        // Parse hour counts — an OBJECT with string keys "0"-"23", not an array
        let mut hour_counts = [0u32; 24];
        if let Some(obj) = json.get("hourCounts").and_then(|v| v.as_object()) {
            for (key, val) in obj {
                if let Ok(hour) = key.parse::<usize>() {
                    if hour < 24 {
                        hour_counts[hour] = val.as_u64().unwrap_or(0) as u32;
                    }
                }
            }
        }

        Ok(StatsCache {
            last_computed_date,
            daily_activity,
            model_usage,
            total_sessions,
            total_messages,
            longest_session,
            first_session_date,
            hour_counts,
        })
    }

    fn stats_path() -> eyre::Result<PathBuf> {
        let home = std::env::var("HOME")
            .map_err(|_| eyre::eyre!("HOME not set"))?;
        Ok(PathBuf::from(home).join(".claude").join("stats-cache.json"))
    }
}
