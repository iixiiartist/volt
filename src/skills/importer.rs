use std::path::Path;

/// Source platform auto-detected from file name/path
#[derive(Debug, Clone, PartialEq)]
pub enum SourceFormat {
    /// CLAUDE.md — project instructions for Claude Code
    Claude,
    /// .cursorrules — project-specific rules for Cursor editor
    Cursor,
    /// copilot-instructions.md — GitHub Copilot instructions
    Copilot,
    /// OpenCode skill (has frontmatter with compatibility: opencode)
    OpenCode,
    /// Generic markdown (ChatGPT GPT instructions, custom rules, etc.)
    Markdown,
    /// Native Volt SKILL.md (already has Volt-native frontmatter)
    Volt,
}

/// Detect the source format from file path and content.
pub fn detect_format(path: &Path, content: &str) -> SourceFormat {
    let fname = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();
    let path_str = path.to_string_lossy().to_lowercase();

    // Check frontmatter first
    if content.trim().starts_with("---") {
        if let Some(frontmatter) = extract_frontmatter(content) {
            if frontmatter.contains("compatibility: opencode")
                || frontmatter.contains("compatibility: \"opencode\"")
            {
                return SourceFormat::OpenCode;
            }
        }
        return SourceFormat::Volt;
    }

    if fname == "claude.md" || path_str.contains("claude") {
        SourceFormat::Claude
    } else if fname == ".cursorrules" || path_str.contains(".cursorrules") {
        SourceFormat::Cursor
    } else if path_str.contains("copilot-instructions") {
        SourceFormat::Copilot
    } else {
        SourceFormat::Markdown
    }
}

/// Extract frontmatter content between --- markers.
fn extract_frontmatter(content: &str) -> Option<&str> {
    let trimmed = content.trim();
    if !trimmed.starts_with("---") {
        return None;
    }
    let after_first = trimmed.strip_prefix("---")?;
    let end = after_first.find("---")?;
    Some(after_first[..end].trim())
}

/// Extract a frontmatter field value by key (e.g. "name", "description").
pub fn extract_frontmatter_field<'a>(content: &'a str, field: &str) -> Option<&'a str> {
    let frontmatter = extract_frontmatter(content)?;
    for line in frontmatter.lines() {
        if let Some((key, value)) = line.split_once(':') {
            if key.trim() == field {
                return Some(value.trim().trim_matches('"').trim_matches('\''));
            }
        }
    }
    None
}

/// Get the body content after frontmatter, or the full content if no frontmatter.
fn strip_frontmatter(content: &str) -> &str {
    let trimmed = content.trim();
    if !trimmed.starts_with("---") {
        return trimmed;
    }
    let after_first = trimmed.strip_prefix("---").unwrap_or(trimmed);
    if let Some(end) = after_first.find("---") {
        after_first[end + 3..].trim()
    } else {
        trimmed
    }
}

/// Generate a skill name from filename (strip extension, clean up).
pub fn name_from_filename(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("imported-skill");
    let cleaned: String = stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('.').to_string();
    if trimmed.is_empty() {
        "imported-skill".into()
    } else {
        trimmed.to_lowercase()
    }
}

/// Extract a description from content (first heading or first non-empty line).
pub fn extract_description(content: &str) -> String {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            return trimmed.trim_start_matches("# ").trim().to_string();
        }
        if trimmed.starts_with("## ") {
            return trimmed.trim_start_matches("## ").trim().to_string();
        }
        if trimmed.starts_with("### ") {
            return trimmed.trim_start_matches("### ").trim().to_string();
        }
    }
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with("---") {
            return trimmed.chars().take(120).collect();
        }
    }
    "Imported skill".into()
}

/// Convert external content to a Volt SKILL.md string with proper frontmatter.
/// Strips any existing frontmatter and regenerates Volt-native frontmatter.
pub fn convert_to_volt_skill(
    path: &Path,
    content: &str,
    format: &SourceFormat,
    name_override: Option<&str>,
) -> String {
    let name = name_override
        .map(|s| s.to_string())
        .or_else(|| {
            // For formats with frontmatter, try to preserve the original name
            match format {
                SourceFormat::OpenCode | SourceFormat::Volt => {
                    extract_frontmatter_field(content, "name").map(|s| s.to_string())
                }
                _ => None,
            }
        })
        .unwrap_or_else(|| name_from_filename(path));

    // Strip existing frontmatter for non-Volt formats
    let body = match format {
        SourceFormat::Volt => content,
        _ => strip_frontmatter(content),
    };

    let description = extract_description(body);

    let mut output = String::new();
    output.push_str("---\n");
    output.push_str(&format!("name: \"{}\"\n", name));
    output.push_str(&format!("version: \"1.0.0\"\n"));
    output.push_str(&format!(
        "description: \"{}\"\n",
        description.replace('"', r#"\""#)
    ));
    output.push_str("mcp_servers: []\n");
    output.push_str("---\n");
    output.push_str(body);

    output
}

/// Get a human-readable label for the source format.
pub fn format_label(fmt: &SourceFormat) -> &'static str {
    match fmt {
        SourceFormat::Claude => "Claude Code (CLAUDE.md)",
        SourceFormat::Cursor => "Cursor (.cursorrules)",
        SourceFormat::Copilot => "GitHub Copilot (copilot-instructions.md)",
        SourceFormat::OpenCode => "OpenCode skill",
        SourceFormat::Markdown => "Generic Markdown",
        SourceFormat::Volt => "Volt SKILL.md",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_detect_claude() {
        let path = Path::new("CLAUDE.md");
        let content = "# Project Rules\n\nBe concise.";
        assert_eq!(detect_format(path, content), SourceFormat::Claude);

        let path = Path::new("path/to/claude.md");
        assert_eq!(detect_format(path, content), SourceFormat::Claude);
    }

    #[test]
    fn test_detect_cursor() {
        let path = Path::new(".cursorrules");
        let content = "You are a Rust expert.";
        assert_eq!(detect_format(path, content), SourceFormat::Cursor);
    }

    #[test]
    fn test_detect_copilot() {
        let path = Path::new(".github/copilot-instructions.md");
        let content = "# Copilot Rules";
        assert_eq!(detect_format(path, content), SourceFormat::Copilot);
    }

    #[test]
    fn test_detect_opencode() {
        let content = "---\nname: code-reviewer\ndescription: Code review automation\ncompatibility: opencode\n---\n# Code Reviewer\n\nAnalyzes PRs.";
        let path = Path::new("skills/code-reviewer/SKILL.md");
        assert_eq!(detect_format(path, content), SourceFormat::OpenCode);
    }

    #[test]
    fn test_detect_volt() {
        let path = Path::new("some-skill.md");
        let content = "---\nname: \"test\"\nversion: \"1.0.0\"\n---\n# Body";
        assert_eq!(detect_format(path, content), SourceFormat::Volt);
    }

    #[test]
    fn test_detect_markdown() {
        let path = Path::new("my-rules.txt");
        let content = "# My Custom Rules";
        assert_eq!(detect_format(path, content), SourceFormat::Markdown);
    }

    #[test]
    fn test_strip_frontmatter_preserves_body() {
        let content = "---\nname: test\n---\n# Body content\n\nMore content.";
        assert_eq!(
            strip_frontmatter(content),
            "# Body content\n\nMore content."
        );
    }

    #[test]
    fn test_strip_frontmatter_no_frontmatter() {
        let content = "# Just body\nNo frontmatter.";
        assert_eq!(strip_frontmatter(content), "# Just body\nNo frontmatter.");
    }

    #[test]
    fn test_name_from_filename() {
        assert_eq!(name_from_filename(Path::new("CLAUDE.md")), "claude");
        assert_eq!(name_from_filename(Path::new(".cursorrules")), "cursorrules");
        assert_eq!(
            name_from_filename(Path::new("my-cool-rules.txt")),
            "my-cool-rules"
        );
        assert_eq!(
            name_from_filename(Path::new("path/to/Copilot-Instructions.md")),
            "copilot-instructions"
        );
    }

    #[test]
    fn test_extract_description_from_heading() {
        let content = "# Project Guidelines\n\nSome rules.";
        assert_eq!(extract_description(content), "Project Guidelines");

        let content = "## Secondary Heading\n\nBody";
        assert_eq!(extract_description(content), "Secondary Heading");
    }

    #[test]
    fn test_extract_description_fallback() {
        let content = "Just some plain text with no heading.";
        assert_eq!(
            extract_description(content),
            "Just some plain text with no heading."
        );
    }

    #[test]
    fn test_convert_opencode_to_volt() {
        let content = "---\nname: code-reviewer\ndescription: Code review\ndescription: Code review automation\ncompatibility: opencode\n---\n# Code Reviewer\n\nAnalyzes PRs for issues.";
        let path = Path::new("code-reviewer/SKILL.md");
        let result = convert_to_volt_skill(path, content, &SourceFormat::OpenCode, None);

        assert!(result.starts_with("---\n"));
        assert!(result.contains("name: \"code-reviewer\""));
        assert!(result.contains("mcp_servers: []"));
        // Frontmatter should be stripped, so only Volt frontmatter present
        assert!(!result.contains("compatibility: opencode"));
        assert!(result.contains("# Code Reviewer\n\nAnalyzes PRs for issues."));
    }

    #[test]
    fn test_convert_claude_to_volt() {
        let path = Path::new("CLAUDE.md");
        let content = "# My Claude Rules\n\nBe concise and correct.";
        let result = convert_to_volt_skill(path, content, &SourceFormat::Claude, None);

        assert!(result.starts_with("---\n"));
        assert!(result.contains("name: \"claude\""));
        assert!(result.contains("description: \"My Claude Rules\""));
        assert!(result.contains("mcp_servers: []"));
        assert!(result.ends_with("# My Claude Rules\n\nBe concise and correct."));
    }

    #[test]
    fn test_convert_with_name_override() {
        let path = Path::new("CLAUDE.md");
        let content = "# Rules";
        let result =
            convert_to_volt_skill(path, content, &SourceFormat::Claude, Some("my-custom-name"));
        assert!(result.contains("name: \"my-custom-name\""));
    }

    #[test]
    fn test_roundtrip_parse_after_convert() {
        use crate::skills::parse_skill_manifest;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-skill.md");
        let content = "# Test Skill\n\nThis is a test.";
        let converted = convert_to_volt_skill(&path, content, &SourceFormat::Markdown, None);

        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(converted.as_bytes()).unwrap();
        drop(f);

        let manifest = parse_skill_manifest(&path).unwrap();
        assert_eq!(manifest.name, "test-skill");
        assert_eq!(manifest.description, "Test Skill");
        assert!(manifest.content.contains("Test Skill"));
    }

    #[test]
    fn test_opencode_roundtrip() {
        use crate::skills::parse_skill_manifest;

        let opencode_content = "---\nname: my-opencode-skill\ndescription: OpenCode skill description\ncompatibility: opencode\n---\n# My Skill\n\nOriginal body content.";
        let path = Path::new("my-opencode-skill/SKILL.md");
        let converted =
            convert_to_volt_skill(path, opencode_content, &SourceFormat::OpenCode, None);

        let dir = tempfile::tempdir().unwrap();
        let tmp_path = dir.path().join("SKILL.md");
        let mut f = std::fs::File::create(&tmp_path).unwrap();
        f.write_all(converted.as_bytes()).unwrap();
        drop(f);

        let manifest = parse_skill_manifest(&tmp_path).unwrap();
        assert_eq!(manifest.name, "my-opencode-skill");
        assert_eq!(manifest.description, "My Skill");
        assert!(manifest.content.contains("Original body content."));
        assert!(!manifest.content.contains("compatibility: opencode"));
    }
}
