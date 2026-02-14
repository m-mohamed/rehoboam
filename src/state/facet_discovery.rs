//! Facet aggregation scanner for ~/.claude/usage-data/facets/
//!
//! Aggregates session quality data from individual facet JSON files
//! into summary statistics: outcomes, helpfulness, satisfaction, friction,
//! session types, success patterns, and top categories.

use color_eyre::eyre;
use std::collections::HashMap;
use std::path::PathBuf;

/// Aggregated session quality metrics
#[derive(Debug, Clone)]
pub struct SessionQuality {
    /// Total number of facet sessions scanned
    pub total_sessions: u32,
    /// Outcome counts: [fully_achieved, mostly_achieved, partially_achieved, not_achieved, other]
    pub outcomes: [u32; 5],
    /// Helpfulness ratings sorted desc by count (e.g., "essential", "very_helpful")
    pub helpfulness: Vec<(String, u32)>,
    /// Top goal categories sorted desc by count, capped at 10
    pub top_categories: Vec<(String, u32)>,
    /// User satisfaction counts sorted desc (e.g., "likely_satisfied", "satisfied")
    pub satisfaction: Vec<(String, u32)>,
    /// Friction type counts sorted desc (e.g., "buggy_code", "wrong_approach")
    pub friction: Vec<(String, u32)>,
    /// Session type counts sorted desc (e.g., "single_task", "multi_task")
    pub session_types: Vec<(String, u32)>,
    /// Primary success pattern counts sorted desc (e.g., "multi_file_changes")
    pub success_patterns: Vec<(String, u32)>,
}

pub struct FacetDiscovery;

impl FacetDiscovery {
    /// Aggregate all facet files from ~/.claude/usage-data/facets/
    pub fn scan_facets() -> eyre::Result<SessionQuality> {
        let facets_dir = Self::facets_dir()?;
        if !facets_dir.is_dir() {
            return Ok(SessionQuality {
                total_sessions: 0,
                outcomes: [0; 5],
                helpfulness: Vec::new(),
                top_categories: Vec::new(),
                satisfaction: Vec::new(),
                friction: Vec::new(),
                session_types: Vec::new(),
                success_patterns: Vec::new(),
            });
        }

        let mut total_sessions = 0u32;
        let mut outcomes = [0u32; 5];
        let mut helpfulness_counts: HashMap<String, u32> = HashMap::new();
        let mut category_counts: HashMap<String, u32> = HashMap::new();
        let mut satisfaction_counts: HashMap<String, u32> = HashMap::new();
        let mut friction_counts: HashMap<String, u32> = HashMap::new();
        let mut session_type_counts: HashMap<String, u32> = HashMap::new();
        let mut success_counts: HashMap<String, u32> = HashMap::new();

        let entries = std::fs::read_dir(&facets_dir)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let json: serde_json::Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => continue,
            };

            total_sessions += 1;

            // Parse outcome
            // Actual values: "fully_achieved", "mostly_achieved", "partially_achieved",
            //                 "not_achieved", "unclear_from_transcript"
            if let Some(outcome) = json.get("outcome").and_then(|v| v.as_str()) {
                match outcome {
                    "fully_achieved" => outcomes[0] += 1,
                    "mostly_achieved" => outcomes[1] += 1,
                    "partially_achieved" => outcomes[2] += 1,
                    "not_achieved" => outcomes[3] += 1,
                    _ => outcomes[4] += 1,
                }
            }

            // Parse helpfulness — actual field is "claude_helpfulness"
            if let Some(helpful) = json.get("claude_helpfulness").and_then(|v| v.as_str()) {
                *helpfulness_counts.entry(helpful.to_string()).or_default() += 1;
            }

            // Parse categories — "goal_categories" is an object like
            // {"code_changes": 1, "information_request": 2}
            if let Some(cats) = json.get("goal_categories").and_then(|v| v.as_object()) {
                for (cat_name, count_val) in cats {
                    let count = count_val.as_u64().unwrap_or(1) as u32;
                    *category_counts.entry(cat_name.clone()).or_default() += count;
                }
            }

            // Parse satisfaction — "user_satisfaction_counts" is an object like
            // {"likely_satisfied": 2, "satisfied": 1}
            if let Some(sats) = json.get("user_satisfaction_counts").and_then(|v| v.as_object()) {
                for (sat_name, count_val) in sats {
                    let count = count_val.as_u64().unwrap_or(1) as u32;
                    *satisfaction_counts.entry(sat_name.clone()).or_default() += count;
                }
            }

            // Parse friction — "friction_counts" is an object like
            // {"buggy_code": 1, "wrong_approach": 1}
            if let Some(fricts) = json.get("friction_counts").and_then(|v| v.as_object()) {
                for (fric_name, count_val) in fricts {
                    let count = count_val.as_u64().unwrap_or(1) as u32;
                    *friction_counts.entry(fric_name.clone()).or_default() += count;
                }
            }

            // Parse session type — "session_type" is a string
            if let Some(st) = json.get("session_type").and_then(|v| v.as_str()) {
                *session_type_counts.entry(st.to_string()).or_default() += 1;
            }

            // Parse primary success — "primary_success" is a string
            if let Some(ps) = json.get("primary_success").and_then(|v| v.as_str()) {
                *success_counts.entry(ps.to_string()).or_default() += 1;
            }
        }

        // Sort all by count desc
        let mut helpfulness: Vec<(String, u32)> = helpfulness_counts.into_iter().collect();
        helpfulness.sort_by(|a, b| b.1.cmp(&a.1));

        let mut top_categories: Vec<(String, u32)> = category_counts.into_iter().collect();
        top_categories.sort_by(|a, b| b.1.cmp(&a.1));
        top_categories.truncate(10);

        let mut satisfaction: Vec<(String, u32)> = satisfaction_counts.into_iter().collect();
        satisfaction.sort_by(|a, b| b.1.cmp(&a.1));

        let mut friction: Vec<(String, u32)> = friction_counts.into_iter().collect();
        friction.sort_by(|a, b| b.1.cmp(&a.1));
        friction.truncate(10);

        let mut session_types: Vec<(String, u32)> = session_type_counts.into_iter().collect();
        session_types.sort_by(|a, b| b.1.cmp(&a.1));

        let mut success_patterns: Vec<(String, u32)> = success_counts.into_iter().collect();
        success_patterns.sort_by(|a, b| b.1.cmp(&a.1));

        Ok(SessionQuality {
            total_sessions,
            outcomes,
            helpfulness,
            top_categories,
            satisfaction,
            friction,
            session_types,
            success_patterns,
        })
    }

    fn facets_dir() -> eyre::Result<PathBuf> {
        let home =
            std::env::var("HOME").map_err(|_| eyre::eyre!("HOME not set"))?;
        Ok(PathBuf::from(home)
            .join(".claude")
            .join("usage-data")
            .join("facets"))
    }
}
