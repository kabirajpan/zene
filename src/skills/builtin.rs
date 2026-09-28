use super::skill::{Skill, SkillScope};
use super::parser::SkillParser;

/// Returns the core set of built-in ZENE skills embedded into the binary.
/// Embedded directly from assets/agent/zene/skills/*/SKILL.md at compile time.
pub fn builtin_skills() -> Vec<Skill> {
    const EXPLORE: &str = include_str!("../../../../assets/agent/zene/skills/explore/SKILL.md");
    const PLAN: &str = include_str!("../../../../assets/agent/zene/skills/plan/SKILL.md");
    const IMPLEMENT: &str = include_str!("../../../../assets/agent/zene/skills/implement/SKILL.md");
    const DEBUG: &str = include_str!("../../../../assets/agent/zene/skills/debug/SKILL.md");
    const VERIFY: &str = include_str!("../../../../assets/agent/zene/skills/verify/SKILL.md");
    const REVIEW: &str = include_str!("../../../../assets/agent/zene/skills/review/SKILL.md");

    vec![
        SkillParser::parse_str("assets/agent/zene/skills/explore/SKILL.md", EXPLORE, SkillScope::Global)
            .expect("Valid built-in explore skill"),
        SkillParser::parse_str("assets/agent/zene/skills/plan/SKILL.md", PLAN, SkillScope::Global)
            .expect("Valid built-in plan skill"),
        SkillParser::parse_str("assets/agent/zene/skills/implement/SKILL.md", IMPLEMENT, SkillScope::Global)
            .expect("Valid built-in implement skill"),
        SkillParser::parse_str("assets/agent/zene/skills/debug/SKILL.md", DEBUG, SkillScope::Global)
            .expect("Valid built-in debug skill"),
        SkillParser::parse_str("assets/agent/zene/skills/verify/SKILL.md", VERIFY, SkillScope::Global)
            .expect("Valid built-in verify skill"),
        SkillParser::parse_str("assets/agent/zene/skills/review/SKILL.md", REVIEW, SkillScope::Global)
            .expect("Valid built-in review skill"),
    ]
}
