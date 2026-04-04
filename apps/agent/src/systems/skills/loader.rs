// ============================================================================
// Skill Loader — Filesystem-Based Skill Discovery
//
// Skills live on disk as structured directories:
//
//   data/skills/
//   ├── commit/
//   │   ├── manifest.yaml     # metadata: name, description, when_to_use, mode
//   │   └── prompt.md         # full prompt (loaded on invocation)
//   └── report/
//       ├── manifest.yaml
//       └── prompt.md
//
// Skills are loaded at agent startup by scanning the skills directory.
// Only manifests are read eagerly — prompt content is loaded on demand
// (when the LLM actually invokes the skill). This is Claude Code's
// "目录常驻, 正文按需" pattern applied to skills.
//
// The skill listing in the system prompt is budget-controlled:
// it takes at most 1% of the context window. If there are too many skills,
// descriptions are truncated (bundled skills are never truncated).
// ============================================================================

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// A loaded skill definition.
#[derive(Debug, Clone)]
pub struct SkillDef {
    pub name: String,
    pub description: String,
    pub when_to_use: String,
    pub mode: SkillMode,

    /// Path to the prompt.md file. Content loaded on demand.
    prompt_path: PathBuf,
}

impl SkillDef {
    /// Build a skill definition from manifest-style fields.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        when_to_use: impl Into<String>,
        mode: SkillMode,
        prompt_path: PathBuf,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            when_to_use: when_to_use.into(),
            mode,
            prompt_path,
        }
    }

    /// Load the full prompt content from disk.
    ///
    /// Called when the LLM invokes this skill — NOT at startup.
    /// This keeps memory usage low when there are many skills.
    pub fn load_prompt(&self) -> Result<String> {
        std::fs::read_to_string(&self.prompt_path).context(format!(
            "failed to read skill prompt: {}",
            self.prompt_path.display()
        ))
    }
}

/// How a skill is executed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillMode {
    /// Prompt injected into current conversation.
    Inline,
    /// Runs in a sub-agent with isolated context.
    Forked,
}

/// manifest.yaml schema.
#[derive(Debug, Deserialize)]
struct Manifest {
    name: String,
    description: String,
    #[serde(default)]
    when_to_use: String,
    #[serde(default = "default_mode")]
    mode: SkillMode,
}

fn default_mode() -> SkillMode {
    SkillMode::Inline
}

/// The skill loader. Scans a directory for skill definitions.
pub struct SkillLoader {
    skills_dir: PathBuf,
    /// Cached skill manifests. Loaded once at startup.
    cache: tokio::sync::OnceCell<Vec<SkillDef>>,
}

impl SkillLoader {
    pub fn new(skills_dir: impl Into<PathBuf>) -> Self {
        Self {
            skills_dir: skills_dir.into(),
            cache: tokio::sync::OnceCell::new(),
        }
    }

    /// Load all skills from disk. Cached after first call.
    pub async fn load_all(&self) -> Result<Vec<SkillDef>> {
        let skills = self
            .cache
            .get_or_try_init(|| async { scan_skills_dir(&self.skills_dir) })
            .await?;
        Ok(skills.clone())
    }

    /// Find a skill by name and load its full prompt.
    pub async fn get_with_prompt(&self, name: &str) -> Result<Option<(SkillDef, String)>> {
        let skills = self.load_all().await?;
        match skills.into_iter().find(|s| s.name == name) {
            Some(skill) => {
                let prompt = skill.load_prompt()?;
                Ok(Some((skill, prompt)))
            }
            None => Ok(None),
        }
    }
}

/// Scan the skills directory for manifest.yaml files.
///
/// Each subdirectory with a manifest.yaml is a skill.
/// Directories without a manifest are silently skipped.
fn scan_skills_dir(dir: &Path) -> Result<Vec<SkillDef>> {
    if !dir.exists() {
        tracing::warn!(dir = %dir.display(), "skills directory not found, no skills loaded");
        return Ok(Vec::new());
    }

    let mut skills = Vec::new();

    let entries = std::fs::read_dir(dir).context(format!(
        "failed to read skills directory: {}",
        dir.display()
    ))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let manifest_path = path.join("manifest.yaml");
        if !manifest_path.exists() {
            tracing::debug!(dir = %path.display(), "skipping directory without manifest.yaml");
            continue;
        }

        match load_skill(&path, &manifest_path) {
            Ok(skill) => {
                tracing::info!(name = skill.name, "skill loaded");
                skills.push(skill);
            }
            Err(e) => {
                tracing::warn!(
                    dir = %path.display(),
                    error = %e,
                    "failed to load skill, skipping"
                );
            }
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    tracing::info!(count = skills.len(), "skills loaded from disk");

    Ok(skills)
}

/// Load a single skill from its directory.
fn load_skill(skill_dir: &Path, manifest_path: &Path) -> Result<SkillDef> {
    let manifest_text =
        std::fs::read_to_string(manifest_path).context("failed to read manifest.yaml")?;

    let manifest: Manifest =
        serde_yaml::from_str(&manifest_text).context("failed to parse manifest.yaml")?;

    let prompt_path = skill_dir.join("prompt.md");
    if !prompt_path.exists() {
        anyhow::bail!("skill '{}' has no prompt.md", manifest.name);
    }

    Ok(SkillDef::new(
        manifest.name,
        manifest.description,
        manifest.when_to_use,
        manifest.mode,
        prompt_path,
    ))
}

// ============================================================================
// Listing Formatting — Budget-Controlled Skill Directory
// ============================================================================

/// Percentage of context window for the skill listing.
const SKILL_BUDGET_PERCENT: f64 = 0.01;
const CHARS_PER_TOKEN: usize = 4;
const MAX_DESC_CHARS: usize = 250;
const DEFAULT_CHAR_BUDGET: usize = 8_000;

/// Format skills into a listing for the system prompt.
///
/// Budget-controlled: total size ≤ 1% of context window.
/// If over budget, descriptions are truncated.
pub fn format_skill_listing(
    skills: &[SkillDef],
    context_window_tokens: Option<usize>,
) -> Option<String> {
    if skills.is_empty() {
        return None;
    }

    let budget = context_window_tokens
        .map(|t| (t as f64 * CHARS_PER_TOKEN as f64 * SKILL_BUDGET_PERCENT) as usize)
        .unwrap_or(DEFAULT_CHAR_BUDGET);

    // Try full descriptions.
    let full_lines: Vec<String> = skills.iter().map(format_line).collect();
    let full_size: usize = full_lines.iter().map(|l| l.len() + 1).sum();

    if full_size <= budget {
        return Some(full_lines.join("\n"));
    }

    // Over budget — compute max description length per skill.
    let name_overhead: usize = skills.iter().map(|s| s.name.len() + 4).sum(); // "- name: "
    let desc_budget = budget.saturating_sub(name_overhead);
    let max_per_skill = desc_budget / skills.len();

    if max_per_skill < 10 {
        // Extreme: name-only listing.
        let lines: Vec<String> = skills.iter().map(|s| format!("- {}", s.name)).collect();
        return Some(lines.join("\n"));
    }

    let lines: Vec<String> = skills
        .iter()
        .map(|s| {
            let desc = skill_description(s);
            let truncated = if desc.len() > max_per_skill {
                format!("{}…", &desc[..max_per_skill.saturating_sub(1)])
            } else {
                desc
            };
            format!("- {}: {truncated}", s.name)
        })
        .collect();

    Some(lines.join("\n"))
}

fn format_line(skill: &SkillDef) -> String {
    let desc = skill_description(skill);
    let desc = if desc.len() > MAX_DESC_CHARS {
        format!("{}…", &desc[..MAX_DESC_CHARS - 1])
    } else {
        desc
    };
    format!("- {}: {desc}", skill.name)
}

fn skill_description(skill: &SkillDef) -> String {
    if skill.when_to_use.is_empty() {
        skill.description.clone()
    } else {
        format!("{} - {}", skill.description, skill.when_to_use)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_skill(dir: &Path, name: &str) {
        let skill_dir = dir.join(name);
        std::fs::create_dir_all(&skill_dir).unwrap();

        let manifest = format!(
            "name: {name}\ndescription: Test skill {name}\nwhen_to_use: When testing\nmode: inline"
        );
        std::fs::write(skill_dir.join("manifest.yaml"), manifest).unwrap();
        std::fs::write(
            skill_dir.join("prompt.md"),
            format!("# {name}\nDo the thing."),
        )
        .unwrap();
    }

    #[test]
    fn test_scan_skills_dir() {
        let dir = TempDir::new().unwrap();
        create_test_skill(dir.path(), "alpha");
        create_test_skill(dir.path(), "beta");

        let skills = scan_skills_dir(dir.path()).unwrap();
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "alpha"); // sorted
        assert_eq!(skills[1].name, "beta");
    }

    #[test]
    fn test_load_prompt_on_demand() {
        let dir = TempDir::new().unwrap();
        create_test_skill(dir.path(), "test");

        let skills = scan_skills_dir(dir.path()).unwrap();
        let prompt = skills[0].load_prompt().unwrap();
        assert!(prompt.contains("# test"));
    }

    #[test]
    fn test_missing_prompt_md_errors() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("broken");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("manifest.yaml"),
            "name: broken\ndescription: No prompt\n",
        )
        .unwrap();
        // No prompt.md → should error.

        let result = scan_skills_dir(dir.path()).unwrap();
        assert!(result.is_empty()); // Skipped with warning.
    }

    #[test]
    fn test_format_skill_listing_within_budget() {
        let dir = TempDir::new().unwrap();
        create_test_skill(dir.path(), "alpha");
        let skills = scan_skills_dir(dir.path()).unwrap();

        let listing = format_skill_listing(&skills, Some(200_000)).unwrap();
        assert!(listing.contains("- alpha:"));
    }

    #[test]
    fn test_nonexistent_dir_returns_empty() {
        let skills = scan_skills_dir(Path::new("/nonexistent/path")).unwrap();
        assert!(skills.is_empty());
    }
}
