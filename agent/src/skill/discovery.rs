// ============================================================================
// Skill Discovery — Intent-Based Recommendation
//
// Runs asynchronously in the background while the agent processes a turn.
// Analyzes the user's message and recommends relevant skills.
//
// Unlike ToolSearch (which the LLM actively queries), skill discovery is
// PROACTIVE — the system suggests skills the LLM might not know about.
//
// Flow:
//   1. User sends message
//   2. Discovery runs in background (tokio::spawn, doesn't block the turn)
//   3. If matches found, injected as <system-reminder> in the next turn
//   4. LLM sees the recommendation and may invoke the skill
//
// This is an optimization, not a requirement. The agent works fine without it.
// When discovery is not available, the LLM relies on the skill listing in
// the system prompt to decide which skills to use.
// ============================================================================

#![allow(dead_code)]

use crate::skill::loader::SkillDef;

/// A skill recommendation from the discovery system.
#[derive(Debug, Clone)]
pub struct SkillRecommendation {
    pub skill_name: String,
    pub reason: String,
}

/// Match user intent against available skills.
///
/// Simple keyword-based matching for now. Can be upgraded to
/// semantic matching (via embeddings) or LLM-based classification
/// when the cost/benefit justifies it.
///
/// Returns up to `max_results` recommendations, sorted by relevance.
pub fn discover_skills(
    user_message: &str,
    available_skills: &[SkillDef],
    max_results: usize,
) -> Vec<SkillRecommendation> {
    let message_lower = user_message.to_lowercase();

    let mut scored: Vec<(&SkillDef, u32)> = available_skills
        .iter()
        .map(|skill| {
            let score = score_skill_relevance(skill, &message_lower);
            (skill, score)
        })
        .filter(|(_, score)| *score > 0)
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.truncate(max_results);

    scored
        .into_iter()
        .map(|(skill, _)| SkillRecommendation {
            skill_name: skill.name.clone(),
            reason: skill.when_to_use.clone(),
        })
        .collect()
}

/// Score how relevant a skill is to the user's message.
///
/// Checks the skill's name, description, and when_to_use against
/// keywords in the message. Returns 0 if no match.
fn score_skill_relevance(skill: &SkillDef, message_lower: &str) -> u32 {
    let mut score = 0u32;

    // Skill name mentioned directly (e.g., user says "commit" and skill is "commit").
    if message_lower.contains(&skill.name.to_lowercase()) {
        score += 10;
    }

    // Check when_to_use keywords against the message.
    let when_words: Vec<&str> = skill.when_to_use.split_whitespace().collect();
    for word in &when_words {
        let word_lower = word.to_lowercase();
        // Skip common words that would match too broadly.
        if word_lower.len() < 4 {
            continue;
        }
        if message_lower.contains(&word_lower) {
            score += 2;
        }
    }

    score
}

/// Format skill recommendations as a `<system-reminder>` for injection.
pub fn format_recommendations(recommendations: &[SkillRecommendation]) -> Option<String> {
    if recommendations.is_empty() {
        return None;
    }

    let lines: Vec<String> = recommendations
        .iter()
        .map(|r| format!("- {}: {}", r.skill_name, r.reason))
        .collect();

    Some(crate::core::xml::system_reminder(&format!(
        "Based on your request, these skills may help:\n{}",
        lines.join("\n")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill::loader::SkillMode;
    use std::fs;

    fn test_skills() -> Vec<SkillDef> {
        let base = std::env::temp_dir().join(format!("skill-discovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&base).unwrap();
        let commit_prompt = base.join("commit.md");
        let report_prompt = base.join("report.md");
        fs::write(&commit_prompt, "commit prompt").unwrap();
        fs::write(&report_prompt, "report prompt").unwrap();

        vec![
            SkillDef::new(
                "commit",
                "Create a git commit",
                "When the user wants to commit changes to git",
                SkillMode::Inline,
                commit_prompt,
            ),
            SkillDef::new(
                "report",
                "Generate a report",
                "When the user asks for analytics or data reports",
                SkillMode::Forked,
                report_prompt,
            ),
        ]
    }

    #[test]
    fn test_discover_direct_name_match() {
        let skills = test_skills();
        let results = discover_skills("please commit my changes", &skills, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].skill_name, "commit");
    }

    #[test]
    fn test_discover_keyword_match() {
        let skills = test_skills();
        let results = discover_skills("I need some analytics on our sales data", &skills, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].skill_name, "report");
    }

    #[test]
    fn test_discover_no_match() {
        let skills = test_skills();
        let results = discover_skills("what is the weather today", &skills, 5);
        assert!(results.is_empty());
    }

    #[test]
    fn test_format_recommendations() {
        let recs = vec![SkillRecommendation {
            skill_name: "commit".into(),
            reason: "Commit changes".into(),
        }];
        let result = format_recommendations(&recs).unwrap();
        assert!(result.contains("<system-reminder>"));
        assert!(result.contains("- commit: Commit changes"));
    }
}
