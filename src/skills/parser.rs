use std::path::Path;

use serde::Deserialize;

use super::{Skill, SkillError, SkillScope};

#[derive(Debug, Deserialize)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

pub struct SkillParser;

impl SkillParser {
    pub fn parse_file(path: impl AsRef<Path>, scope: SkillScope) -> Result<Skill, SkillError> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)?;
        Self::parse_str(path, &content, scope)
    }

    pub fn parse_str(path: impl AsRef<Path>, content: &str, scope: SkillScope) -> Result<Skill, SkillError> {
        let path = path.as_ref();
        let normalized = content.replace("\r\n", "\n");
        let body = normalized.strip_prefix("---\n").ok_or_else(|| {
            SkillError::InvalidFrontmatter("SKILL.md must start with YAML frontmatter".into())
        })?;
        let (frontmatter, instructions) = body.split_once("\n---").ok_or_else(|| {
            SkillError::InvalidFrontmatter("missing closing frontmatter delimiter".into())
        })?;
        let metadata: Frontmatter = serde_yaml::from_str(frontmatter)
            .map_err(|error| SkillError::InvalidFrontmatter(error.to_string()))?;

        let name = required_metadata("name", metadata.name)?;
        let description = required_metadata("description", metadata.description)?;
        let instructions = instructions
            .strip_prefix('\n')
            .unwrap_or(instructions)
            .to_string();

        Ok(Skill {
            name,
            description,
            version: metadata.version,
            tags: metadata.tags,
            path: path.to_path_buf(),
            instructions,
            scope,
        })
    }
}

fn required_metadata(field: &str, value: Option<String>) -> Result<String, SkillError> {
    let value = value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| SkillError::InvalidMetadata(format!("missing required field `{}`", field)))?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "---\nname: debugging\ndescription: Diagnose failures.\nversion: \"1.0.0\"\ntags:\n  - errors\n  - testing\n---\n# Debugging\n\nInvestigate carefully.\n";

    #[test]
    fn parses_metadata_and_markdown_body() {
        let skill = SkillParser::parse_str("debugging/SKILL.md", VALID, SkillScope::Project).unwrap();
        assert_eq!(skill.name, "debugging");
        assert_eq!(skill.description, "Diagnose failures.");
        assert_eq!(skill.version.as_deref(), Some("1.0.0"));
        assert_eq!(skill.tags, vec!["errors", "testing"]);
        assert_eq!(skill.scope, SkillScope::Project);
        assert!(skill.instructions.contains("# Debugging"));
    }

    #[test]
    fn rejects_missing_frontmatter() {
        let error = SkillParser::parse_str("SKILL.md", "# Missing", SkillScope::Project).unwrap_err();
        assert!(error.to_string().contains("must start with YAML frontmatter"));
    }

    #[test]
    fn rejects_invalid_yaml() {
        let error = SkillParser::parse_str("SKILL.md", "---\nname: [\n---\nbody", SkillScope::Project).unwrap_err();
        assert!(matches!(error, SkillError::InvalidFrontmatter(_)));
    }

    #[test]
    fn rejects_missing_required_metadata() {
        let error = SkillParser::parse_str("SKILL.md", "---\nname: debugging\n---\nbody", SkillScope::Project).unwrap_err();
        assert!(error.to_string().contains("description"));
    }
}
