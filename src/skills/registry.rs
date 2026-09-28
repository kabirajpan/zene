use std::collections::BTreeMap;

use super::{Skill, SkillError, SkillScope};

#[derive(Debug, Default, Clone)]
pub struct SkillRegistry {
    skills: BTreeMap<String, Skill>,
}

impl SkillRegistry {
    /// Register a skill. If a skill with the same name exists:
    /// - Higher scope overrides lower scope (Project > User > Global).
    /// - Equal scope returns DuplicateName error.
    /// - Lower scope is ignored in favor of the existing higher-scope skill.
    pub fn register(&mut self, skill: Skill) -> Result<(), SkillError> {
        if let Some(existing) = self.skills.get(&skill.name) {
            if skill.scope > existing.scope {
                self.skills.insert(skill.name.clone(), skill);
                return Ok(());
            } else if skill.scope == existing.scope {
                return Err(SkillError::DuplicateName(skill.name));
            } else {
                return Ok(());
            }
        }
        self.skills.insert(skill.name.clone(), skill);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    pub fn list(&self) -> impl Iterator<Item = &Skill> {
        self.skills.values()
    }

    pub fn list_by_scope(&self, scope: SkillScope) -> impl Iterator<Item = &Skill> {
        self.skills.values().filter(move |s| s.scope == scope)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.skills.contains_key(name)
    }

    /// Compact summary of available skills for injection into the system prompt.
    pub fn prompt_summary(&self) -> String {
        let mut summary = String::new();
        for skill in self.skills.values() {
            summary.push_str(&format!("- {}: {}\n", skill.name, skill.description));
        }
        summary
    }

    pub fn unregister(&mut self, name: &str) -> Option<Skill> {
        self.skills.remove(name)
    }

    pub fn len(&self) -> usize {
        self.skills.len()
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill_with_scope(name: &str, scope: SkillScope) -> Skill {
        Skill {
            name: name.into(),
            description: "description".into(),
            version: None,
            tags: Vec::new(),
            path: name.into(),
            instructions: "instructions".into(),
            scope,
        }
    }

    #[test]
    fn manages_skills_without_overwriting_same_scope_duplicates() {
        let mut registry = SkillRegistry::default();
        registry
            .register(skill_with_scope("debugging", SkillScope::Global))
            .unwrap();
        assert!(registry.contains("debugging"));
        assert_eq!(registry.get("debugging").unwrap().name, "debugging");
        assert_eq!(registry.list().count(), 1);
        assert!(matches!(
            registry.register(skill_with_scope("debugging", SkillScope::Global)),
            Err(SkillError::DuplicateName(_))
        ));
    }

    #[test]
    fn higher_scope_overrides_lower_scope() {
        let mut registry = SkillRegistry::default();
        registry
            .register(skill_with_scope("test", SkillScope::Global))
            .unwrap();
        assert_eq!(registry.get("test").unwrap().scope, SkillScope::Global);

        // User overrides Global
        registry
            .register(skill_with_scope("test", SkillScope::User))
            .unwrap();
        assert_eq!(registry.get("test").unwrap().scope, SkillScope::User);

        // Project overrides User
        registry
            .register(skill_with_scope("test", SkillScope::Project))
            .unwrap();
        assert_eq!(registry.get("test").unwrap().scope, SkillScope::Project);

        // Global cannot override Project
        registry
            .register(skill_with_scope("test", SkillScope::Global))
            .unwrap();
        assert_eq!(registry.get("test").unwrap().scope, SkillScope::Project);
    }
}
