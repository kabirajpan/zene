pub mod gemini;
pub mod groq;

use std::fs;
use std::path::Path;
use crate::traits::provider::Provider;

pub use gemini::GeminiProvider;
pub use groq::GroqProvider;

/// Simple helper to get an API key either from environment variables
/// or by looking for a `.env` file in current working directory and parent directories.
pub fn get_key(name: &str) -> Option<String> {
    if let Ok(val) = std::env::var(name) {
        if !val.trim().is_empty() {
            return Some(val.trim().to_string());
        }
    }

    // Try finding .env file only in the current directory
    let candidate_paths = [
        "./.env",
    ];

    for path_str in &candidate_paths {
        let path = Path::new(path_str);
        if let Ok(content) = fs::read_to_string(path) {
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
