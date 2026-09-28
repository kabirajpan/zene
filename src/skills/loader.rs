use std::path::Path;

use super::{Skill, SkillError, SkillParser, SkillScope};

pub struct SkillLoader;

impl SkillLoader {
    pub fn load(path: impl AsRef<Path>, scope: SkillScope) -> Result<Skill, SkillError> {
        SkillParser::parse_file(path, scope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_missing_skill_file() {
        let error = SkillLoader::load("missing/SKILL.md", SkillScope::Project).unwrap_err();
        assert!(matches!(error, SkillError::Io(_)));
    }
}
