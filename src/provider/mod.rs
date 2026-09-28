pub mod gemini;
pub mod groq;

use std::fs;
use std::path::Path;
use crate::traits::provider::Provider;

pub use gemini::GeminiProvider;
pub use groq::GroqProvider;

/// Simple helper to get an API key either from environment variables
/// or by looking for a `.env` file in current directory, parent directories,
/// or user configuration directories (~/.config/zene/.env, ~/.zene/.env, ~/.zenthree/.env).
pub fn get_key(name: &str) -> Option<String> {
    if let Ok(val) = std::env::var(name) {
        if !val.trim().is_empty() {
            return Some(val.trim().to_string());
        }
    }

    let parse_env_file = |path: &Path| -> Option<String> {
        let content = fs::read_to_string(path).ok()?;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || !trimmed.contains('=') {
                continue;
            }
            let mut parts = trimmed.splitn(2, '=');
            let key = parts.next().unwrap_or("").trim();
            let val = parts.next().unwrap_or("").trim().trim_matches('"').trim_matches('\'');
            if key == name && !val.is_empty() {
                return Some(val.to_string());
            }
        }
        None
    };

    // 1. Current working directory and climb parent directories (up to 4 levels)
    if let Ok(mut current) = std::env::current_dir() {
        for _ in 0..5 {
            let env_path = current.join(".env");
            if let Some(val) = parse_env_file(&env_path) {
                return Some(val);
            }
            if !current.pop() {
                break;
            }
        }
    }

    // 2. Global user configuration directories
    if let Some(config_dir) = dirs::config_dir() {
        let path = config_dir.join("zene").join(".env");
        if let Some(val) = parse_env_file(&path) {
            return Some(val);
        }
    }

    if let Some(home) = dirs::home_dir() {
        let zene_home_env = home.join(".zene").join(".env");
        if let Some(val) = parse_env_file(&zene_home_env) {
            return Some(val);
        }

        let zenthree_home_env = home.join(".zenthree").join(".env");
        if let Some(val) = parse_env_file(&zenthree_home_env) {
            return Some(val);
        }
    }

    None
}

/// Run completion with Groq
pub fn complete_groq(prompt: &str) -> Result<String, String> {
    let key = get_key("GROQ_API_KEY").ok_or_else(|| "GROQ_API_KEY not found in env or .env".to_string())?;
    groq::chat_completion(&key, prompt)
}

/// Run completion with Gemini
pub fn complete_gemini(prompt: &str) -> Result<String, String> {
    let key = get_key("GEMINI_API_KEY").ok_or_else(|| "GEMINI_API_KEY not found in env or .env".to_string())?;
    gemini::generate_content(&key, prompt)
}

/// Run completion trying Groq first (fastest), falling back to Gemini if Groq fails or key is missing.
pub fn complete(prompt: &str) -> Result<String, String> {
    if get_key("GROQ_API_KEY").is_some() {
        match complete_groq(prompt) {
            Ok(res) => return Ok(res),
            Err(groq_err) => {
                if get_key("GEMINI_API_KEY").is_some() {
                    return complete_gemini(prompt).map_err(|gem_err| {
                        format!("Groq failed: {groq_err}\nGemini fallback also failed: {gem_err}")
                    });
                } else {
                    return Err(groq_err);
                }
            }
        }
    }

    if get_key("GEMINI_API_KEY").is_some() {
        return complete_gemini(prompt);
    }

    Err("Neither GROQ_API_KEY nor GEMINI_API_KEY found in .env or environment".to_string())
}

/// Automatically instantiate a Provider trait object from available API keys.
/// Prefers Gemini (1,000,000 TPM free tier) over Groq to avoid rate limiting.
/// Falls back to Groq if GEMINI_API_KEY is not set.
pub fn create_default_provider() -> Option<Box<dyn Provider>> {
    if let Some(key) = get_key("GROQ_API_KEY") {
        return Some(Box::new(GroqProvider::new(key)));
    }
    if let Some(key) = get_key("GEMINI_API_KEY") {
        return Some(Box::new(GeminiProvider::new(key)));
    }
    None
}

/// Instantiate a specific provider by name + model ID.
/// `provider_name`: `"gemini"` or `"groq"` (case-insensitive).
/// Falls back to `create_default_provider()` if the requested key is missing.
pub fn create_provider_for(provider_name: &str, model_id: &str) -> Option<Box<dyn Provider>> {
    match provider_name.to_lowercase().as_str() {
        "gemini" => {
            let key = get_key("GEMINI_API_KEY")?;
            Some(Box::new(GeminiProvider::with_model(key, model_id)))
        }
        "groq" => {
            let key = get_key("GROQ_API_KEY")?;
            Some(Box::new(GroqProvider::with_model(key, model_id)))
        }
        _ => create_default_provider(),
    }
}
