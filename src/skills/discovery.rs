use std::path::{Path, PathBuf};

use super::{SkillError, SkillLoader, SkillRegistry, SkillScope};

#[derive(Debug, Default)]
pub struct DiscoveryReport {
    pub registry: SkillRegistry,
    pub errors: Vec<(PathBuf, SkillError)>,
}

/// Discover skills across the 3-tier hierarchy:
/// 1. Global (Built-in core skills embedded in the binary)
/// 2. User base (~/.zenthree/skills/ in the user home directory)
/// 3. Project base (<workspace>/.zenthree/skills/ in the project repository)
///
/// Project skills take precedence over User skills, which take precedence over Global skills.
pub fn discover(workspace: impl AsRef<Path>) -> Result<DiscoveryReport, SkillError> {
    let mut report = DiscoveryReport::default();

    // Tier 1: Global built-in skills
    for skill in super::builtin_skills() {
        let _ = report.registry.register(skill);
    }

    // Tier 2: User base skills from home directory (~/.zene/skills or ~/.zenthree/skills)
    if let Some(home) = dirs::home_dir() {
        let zene_user_dir = home.join(".zene").join("skills");
        if zene_user_dir.exists() {
            scan_directory(&zene_user_dir, SkillScope::User, &mut report);
        }

        let user_skills_dir = home.join(".zenthree").join("skills");
        scan_directory(&user_skills_dir, SkillScope::User, &mut report);

        let legacy_user_dir = home.join(".zenthree").join("agent").join("zene").join("skills");
        if legacy_user_dir.exists() && legacy_user_dir != user_skills_dir {
            scan_directory(&legacy_user_dir, SkillScope::User, &mut report);
        }
    }

    // Tier 3: Project base skills from workspace (.zene/skills or .zenthree/skills)
    let ws = workspace.as_ref();
    let zene_project_dir = ws.join(".zene").join("skills");
    if zene_project_dir.exists() {
        scan_directory(&zene_project_dir, SkillScope::Project, &mut report);
    }

    let project_skills_dir = ws.join(".zenthree").join("skills");
    scan_directory(&project_skills_dir, SkillScope::Project, &mut report);

    let legacy_project_dir = ws.join(".zenthree").join("agent").join("zene").join("skills");
    if legacy_project_dir.exists() && legacy_project_dir != project_skills_dir {
        scan_directory(&legacy_project_dir, SkillScope::Project, &mut report);
    }

    Ok(report)
}

fn scan_directory(dir: &Path, scope: SkillScope, report: &mut DiscoveryReport) {
    if !dir.exists() {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) => {
            report.errors.push((dir.to_path_buf(), SkillError::Io(err)));
            return;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                report.errors.push((dir.to_path_buf(), SkillError::Io(err)));
                continue;
            }
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let skill_path = path.join("SKILL.md");
        if !skill_path.is_file() {
            report.errors.push((
                skill_path,
                SkillError::MissingSkill(path.display().to_string()),
            ));
            continue;
        }

        match SkillLoader::load(&skill_path, scope) {
            Ok(skill) => {
                if let Err(error) = report.registry.register(skill) {
                    report.errors.push((skill_path, error));
                }
            }
            Err(error) => report.errors.push((skill_path, error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn workspace() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("agent-skills-{}", id));
        fs::create_dir_all(
            path.join(".zenthree")
                .join("agent")
                .join("zene")
                .join("skills"),
        )
        .unwrap();
        path
    }

    fn write_skill(root: &Path, name: &str, content: &str) {
        let dir = root
            .join(".zenthree")
            .join("agent")
            .join("zene")
            .join("skills")
            .join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), content).unwrap();
    }

    #[test]
    fn discovers_valid_skills_and_continues_after_invalid_one() {
        let root = workspace();
        write_skill(&root, "debugging", "---\nname: debugging\ndescription: Debug.\n---\n# Debugging");
        write_skill(&root, "broken", "---\nname: broken\n---\n# Broken");
        let report = discover(&root).unwrap();
        assert!(report.registry.contains("debugging"));
        assert_eq!(report.errors.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_directory_contains_builtin_skills() {
        let root = std::env::temp_dir().join("agent-skills-missing");
        let report = discover(root).unwrap();
        // Builtin core skills are always available
        assert!(report.registry.contains("debug"));
        assert!(report.registry.contains("explore"));
        assert_eq!(report.registry.list_by_scope(SkillScope::Project).count(), 0);
        assert_eq!(report.registry.list_by_scope(SkillScope::Global).count(), 6);
    }

    #[test]
    fn project_skill_overrides_builtin_skill() {
        let root = workspace();
        write_skill(
            &root,
            "debug",
            "---\nname: debug\ndescription: Project specific custom debugging.\n---\n# Custom Debug",
        );
        let report = discover(&root).unwrap();
        let debug_skill = report.registry.get("debug").unwrap();
        assert_eq!(debug_skill.scope, SkillScope::Project);
        assert_eq!(debug_skill.description, "Project specific custom debugging.");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reports_directory_without_skill_file() {
        let root = workspace();
        fs::create_dir_all(
            root.join(".zenthree")
                .join("agent")
                .join("zene")
                .join("skills")
                .join("empty"),
        )
        .unwrap();
        let report = discover(&root).unwrap();
        assert_eq!(report.errors.len(), 1);
        let _ = fs::remove_dir_all(root);
    }
}
