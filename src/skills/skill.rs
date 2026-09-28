use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum SkillScope {
    /// Built-in core skills compiled directly into the binary.
    Global = 0,
    /// User personal skills located in user home ~/.zenthree/skills/.
    User = 1,
    /// Project-specific skills located in <workspace>/.zenthree/skills/.
    Project = 2,
}

impl fmt::Display for SkillScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Global => write!(f, "global"),
            Self::User => write!(f, "user"),
            Self::Project => write!(f, "project"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub tags: Vec<String>,
    pub scope: SkillScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub tags: Vec<String>,
    pub path: PathBuf,
    pub instructions: String,
    pub scope: SkillScope,
}

impl Skill {
    pub fn metadata(&self) -> SkillMetadata {
        SkillMetadata {
            name: self.name.clone(),
            description: self.description.clone(),
            version: self.version.clone(),
            tags: self.tags.clone(),
            scope: self.scope,
        }
    }
}

#[derive(Debug)]
pub enum SkillError {
    Io(std::io::Error),
    InvalidFrontmatter(String),
    InvalidMetadata(String),
    DuplicateName(String),
    MissingSkill(String),
}

impl fmt::Display for SkillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "skill I/O error: {}", error),
            Self::InvalidFrontmatter(error) => write!(f, "invalid skill frontmatter: {}", error),
            Self::InvalidMetadata(error) => write!(f, "invalid skill metadata: {}", error),
            Self::DuplicateName(name) => write!(f, "duplicate skill name: {}", name),
            Self::MissingSkill(name) => write!(f, "skill not found: {}", name),
        }
    }
}

impl std::error::Error for SkillError {}

impl From<std::io::Error> for SkillError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
