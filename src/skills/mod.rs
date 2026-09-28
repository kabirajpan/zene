pub mod builtin;
mod discovery;
mod loader;
mod parser;
mod registry;
mod skill;

pub use builtin::builtin_skills;
pub use discovery::{discover, DiscoveryReport};
pub use loader::SkillLoader;
pub use parser::SkillParser;
pub use registry::SkillRegistry;
pub use skill::{Skill, SkillError, SkillMetadata, SkillScope};
