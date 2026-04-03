// ============================================================================
// Conditional Rules
//
// Rules are like instructions, but with conditions — they only apply
// when certain context is present. Claude Code uses `paths:` frontmatter
// to match rules to file operations. Our cloud version generalizes this:
//
//   Unconditional rules: always injected (like instructions but managed
//   separately — think "guardrails" vs "preferences").
//
//   Conditional rules: injected only when a condition matches.
//   Conditions can be topic-based, tool-based, or custom.
//
// Rules live below the cache boundary. Unconditional rules are always
// in the dynamic zone. Conditional rules are injected as <system-reminder>
// during the turn when their condition fires.
// ============================================================================

#![allow(dead_code)]

/// A rule definition.
#[derive(Debug, Clone)]
pub struct Rule {
    /// Human-readable name for logging.
    pub name: String,

    /// The rule content — instructions for the LLM.
    pub content: String,

    /// When this rule applies. None = always (unconditional).
    pub condition: Option<RuleCondition>,
}

/// When a conditional rule should be activated.
#[derive(Debug, Clone)]
pub enum RuleCondition {
    /// Rule applies when specific tools are being used.
    /// Example: "When using DbQuery, always add LIMIT clauses."
    ToolMatch { tool_names: Vec<String> },

    /// Rule applies when the conversation topic matches keywords.
    /// Example: "When discussing payments, mention PCI compliance."
    TopicMatch { keywords: Vec<String> },
}

/// Evaluate which conditional rules should be active for the current turn.
///
/// Returns the content of all matching rules, ready for injection.
pub fn evaluate_rules(
    rules: &[Rule],
    active_tool_names: &[&str],
    user_message: &str,
) -> Vec<String> {
    rules
        .iter()
        .filter(|rule| should_activate(rule, active_tool_names, user_message))
        .map(|rule| rule.content.clone())
        .collect()
}

/// Check if a rule's condition is met.
fn should_activate(rule: &Rule, active_tool_names: &[&str], user_message: &str) -> bool {
    let Some(condition) = &rule.condition else {
        // Unconditional rules are always active.
        return true;
    };

    match condition {
        RuleCondition::ToolMatch { tool_names } => tool_names.iter().any(|name| {
            active_tool_names
                .iter()
                .any(|active| active.eq_ignore_ascii_case(name))
        }),
        RuleCondition::TopicMatch { keywords } => {
            let message_lower = user_message.to_lowercase();
            keywords
                .iter()
                .any(|kw| message_lower.contains(&kw.to_lowercase()))
        }
    }
}

/// Format active rules as a `<system-reminder>` for conversation injection.
///
/// Only used for conditional rules that were activated this turn.
/// Unconditional rules are injected directly into the system prompt.
pub fn format_conditional_rules(rules: &[String]) -> Option<String> {
    if rules.is_empty() {
        return None;
    }

    let content = format!("Active rules for this turn:\n\n{}", rules.join("\n\n"));

    Some(crate::core::xml::system_reminder(&content))
}

/// Separate rules into unconditional and conditional.
pub fn partition_rules(rules: &[Rule]) -> (Vec<&Rule>, Vec<&Rule>) {
    let mut unconditional = Vec::new();
    let mut conditional = Vec::new();

    for rule in rules {
        if rule.condition.is_none() {
            unconditional.push(rule);
        } else {
            conditional.push(rule);
        }
    }

    (unconditional, conditional)
}

/// Format unconditional rules for direct system prompt injection.
pub fn format_unconditional_rules(rules: &[&Rule]) -> Option<String> {
    if rules.is_empty() {
        return None;
    }

    let sections: Vec<String> = rules.iter().map(|r| r.content.clone()).collect();

    Some(format!("# Rules\n\n{}", sections.join("\n\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unconditional_always_active() {
        let rules = vec![Rule {
            name: "always".into(),
            content: "Be polite.".into(),
            condition: None,
        }];
        let result = evaluate_rules(&rules, &[], "anything");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_tool_match_activates() {
        let rules = vec![Rule {
            name: "db-safety".into(),
            content: "Always add LIMIT.".into(),
            condition: Some(RuleCondition::ToolMatch {
                tool_names: vec!["DbQuery".into()],
            }),
        }];

        let active = evaluate_rules(&rules, &["DbQuery"], "");
        assert_eq!(active.len(), 1);

        let inactive = evaluate_rules(&rules, &["WebSearch"], "");
        assert!(inactive.is_empty());
    }

    #[test]
    fn test_topic_match_activates() {
        let rules = vec![Rule {
            name: "pci".into(),
            content: "Mention PCI compliance.".into(),
            condition: Some(RuleCondition::TopicMatch {
                keywords: vec!["payment".into(), "credit card".into()],
            }),
        }];

        let active = evaluate_rules(&rules, &[], "process a payment");
        assert_eq!(active.len(), 1);

        let inactive = evaluate_rules(&rules, &[], "refactor the auth module");
        assert!(inactive.is_empty());
    }

    #[test]
    fn test_partition_rules() {
        let rules = vec![
            Rule {
                name: "a".into(),
                content: "a".into(),
                condition: None,
            },
            Rule {
                name: "b".into(),
                content: "b".into(),
                condition: Some(RuleCondition::TopicMatch {
                    keywords: vec!["x".into()],
                }),
            },
        ];
        let (u, c) = partition_rules(&rules);
        assert_eq!(u.len(), 1);
        assert_eq!(c.len(), 1);
    }
}
