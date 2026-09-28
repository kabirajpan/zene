use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::traits::provider::Provider;
use crate::types::error::ProviderError;
use crate::types::event::ProviderEvent;
use crate::types::message::{Message, Role};

// ─── Simple one-shot helper (kept for provider::complete_gemini) ─────────────

#[derive(Serialize)]
struct Part {
    text: String,
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<Content>,
}

#[derive(Deserialize)]
struct GeminiPart {
    text: Option<String>,
}

#[derive(Deserialize)]
struct GeminiContent {
    parts: Option<Vec<GeminiPart>>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    error: Option<serde_json::Value>,
}

pub fn generate_content(api_key: &str, prompt: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
        api_key.trim()
    );

    let req_body = GeminiRequest {
        contents: vec![Content {
            parts: vec![Part {
                text: prompt.to_string(),
            }],
        }],
    };

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&req_body)
        .send()
        .map_err(|e| format!("Gemini request failed: {e}"))?;

    let status = response.status();
    let resp_text = response
        .text()
        .map_err(|e| format!("Failed to read Gemini response: {e}"))?;

    if !status.is_success() {
        return Err(format!("Gemini API error (status {status}): {resp_text}"));
    }

    let parsed: GeminiResponse = serde_json::from_str(&resp_text)
        .map_err(|e| format!("Failed to parse Gemini response: {e}. Raw: {resp_text}"))?;

    if let Some(candidates) = parsed.candidates {
        if let Some(first) = candidates.into_iter().next() {
            if let Some(content) = first.content {
                if let Some(parts) = content.parts {
                    let text = parts
                        .into_iter()
                        .filter_map(|p| p.text)
                        .collect::<Vec<_>>()
                        .join("");
                    if !text.is_empty() {
                        return Ok(text.trim().to_string());
                    }
                }
            }
        }
    }

    if let Some(err) = parsed.error {
        return Err(format!("Gemini error response: {err}"));
    }

    Err("Empty response from Gemini".to_string())
}

// ─── Full Provider trait implementation with tool-calling ────────────────────

/// Default model for GeminiProvider.
/// `gemini-2.5-flash` has a 1,000,000 TPM free-tier limit and full tool-calling support.
const DEFAULT_GEMINI_MODEL: &str = "gemini-2.5-flash";

/// Gemini LLM backend implementing the unified `Provider` trait.
/// Uses the Gemini `generateContent` endpoint with `tools` + `functionDeclarations`
/// so the orchestrator can call any registered tool.
pub struct GeminiProvider {
    api_key: String,
    model: String,
}

fn parse_gemini_malformed_call(msg: &str) -> Option<(String, Value)> {
    let clean = msg.strip_prefix("Malformed function call:")
        .unwrap_or(msg)
        .trim();
    let unwrapped = if clean.starts_with("print(") && clean.ends_with(')') {
        clean[6..clean.len() - 1].trim()
    } else {
        clean
    };
    // e.g. default_api.run_terminal(command='ls -R') or run_terminal(command='ls -R')
    let paren_pos = unwrapped.find('(')?;
    if !unwrapped.ends_with(')') {
        return None;
    }
    let target = unwrapped[..paren_pos].trim();
    let raw_name = target.split('.').last().unwrap_or(target);
    let inner_args = unwrapped[paren_pos + 1..unwrapped.len() - 1].trim();

    let mut map = serde_json::Map::new();
    for part in inner_args.split(',') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=') {
            let key = k.trim().to_string();
            let val_str = v.trim().trim_matches('\'').trim_matches('"');
            map.insert(key, Value::String(val_str.to_string()));
        }
    }
    Some((raw_name.to_string(), Value::Object(map)))
}

impl GeminiProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: DEFAULT_GEMINI_MODEL.to_string(),
        }
    }

    pub fn with_model(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
        }
    }
}

impl Provider for GeminiProvider {
    fn name(&self) -> &str {
        "Gemini"
    }

    fn complete_stream(
        &self,
        messages: &[Message],
        tools: &[Value],
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<(), ProviderError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model,
            self.api_key.trim()
        );

        // ── Convert messages → Gemini `contents` format & extract `systemInstruction` ──
        let mut system_parts: Vec<String> = Vec::new();
        let mut contents: Vec<Value> = Vec::new();

        for m in messages {
            match m.role {
                Role::System => {
                    system_parts.push(m.content.clone());
                }
                Role::User => {
                    contents.push(json!({
                        "role": "user",
                        "parts": [{ "text": m.content }]
                    }));
                }
                Role::Assistant => {
                    let mut parts: Vec<Value> = Vec::new();
                    if !m.content.is_empty() {
                        parts.push(json!({ "text": m.content }));
                    }
                    if let Some(tool_calls) = &m.tool_calls {
                        for tc in tool_calls {
                            let mut fc_part = json!({
                                "functionCall": {
                                    "name": tc.name,
                                    "args": tc.args
                                }
                            });
                            if let Some(ref sig) = tc.thought_signature {
                                fc_part["thoughtSignature"] = json!(sig);
                            }
                            parts.push(fc_part);
                        }
                    }
                    if !parts.is_empty() {
                        contents.push(json!({ "role": "model", "parts": parts }));
                    }
                }
                Role::Tool => {
                    // Gemini expects tool results as a "user" turn with functionResponse parts
                    contents.push(json!({
                        "role": "user",
                        "parts": [{
                            "functionResponse": {
                                "name": m.tool_call_id.as_deref().unwrap_or("tool"),
                                "response": { "output": m.content }
                            }
                        }]
                    }));
                }
            }
        }

        // ── Build request body ────────────────────────────────────────────────
        let mut body = json!({ "contents": contents });

        if !tools.is_empty() {
            system_parts.push("When calling tools, invoke the provided function declarations directly. Do NOT output Python code or wrap calls in print() or default_api.".to_string());
        }

        if !system_parts.is_empty() {
            body["systemInstruction"] = json!({
                "parts": [{ "text": system_parts.join("\n\n") }]
            });
        }

        // Convert OpenAI-style tool schemas → Gemini functionDeclarations
        if !tools.is_empty() {
            let function_declarations: Vec<Value> = tools
                .iter()
                .filter_map(|t| {
                    // OpenAI schema: { "type": "function", "function": { "name", "description", "parameters" } }
                    let func = t.get("function")?;
                    Some(json!({
                        "name": func.get("name")?,
                        "description": func.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                        "parameters": func.get("parameters").cloned().unwrap_or(json!({}))
                    }))
                })
                .collect();

            if !function_declarations.is_empty() {
                body["tools"] = json!([{ "functionDeclarations": function_declarations }]);
            }
        }

        const MAX_RETRIES: u32 = 3;
        let mut attempts = 0;
        let (status, resp_text) = loop {
            let response = client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .map_err(|e| ProviderError::Network(e.to_string()))?;

            let status = response.status();
            let retry_secs: u64 = if status.as_u16() == 429 {
                response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|f| (f.ceil() as u64).max(1))
                    .unwrap_or(8)
            } else {
                0
            };

            let resp_text = response
                .text()
                .map_err(|e| ProviderError::Network(e.to_string()))?;

            if status.as_u16() == 429 {
                let is_daily_quota = resp_text.contains("Quota exceeded") || resp_text.contains("RESOURCE_EXHAUSTED");
                if is_daily_quota || retry_secs > 8 || attempts >= MAX_RETRIES {
                    break (status, resp_text);
                }
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_secs(retry_secs.min(5)));
                continue;
            }

            break (status, resp_text);
        };


        if !status.is_success() {
            return Err(ProviderError::Malformed(format!(
                "Gemini API error (status {}): {}",
                status, resp_text
            )));
        }

        // ── Parse response ────────────────────────────────────────────────────
        let parsed: Value = serde_json::from_str(&resp_text)
            .map_err(|e| ProviderError::Malformed(format!("JSON parse error: {e}")))?;

        if let Some(candidates) = parsed.get("candidates").and_then(|c| c.as_array()) {
            if let Some(candidate) = candidates.first() {
                let finish_reason = candidate.get("finishReason").and_then(|r| r.as_str()).unwrap_or("");
                let mut found_call = false;

                if let Some(parts) = candidate
                    .get("content")
                    .and_then(|c| c.get("parts"))
                    .and_then(|p| p.as_array())
                {
                    for part in parts {
                        // Plain text delta
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                on_event(ProviderEvent::TextDelta(text.to_string()));
                            }
                        }

                        // Function/tool call
                        if let Some(fc) = part.get("functionCall") {
                            found_call = true;
                            let name = fc
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let args = fc.get("args").cloned().unwrap_or(json!({}));
                            let id = fc
                                .get("id")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| name.clone());
                            let thought_signature = part
                                .get("thoughtSignature")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            on_event(ProviderEvent::ToolCallComplete {
                                id,
                                name,
                                args,
                                thought_signature,
                            });
                        }
                    }
                }

                // Fallback: If Gemini emitted MALFORMED_FUNCTION_CALL e.g. "print(default_api.run_terminal(command='ls -F'))"
                if !found_call && finish_reason == "MALFORMED_FUNCTION_CALL" {
                    if let Some(msg) = candidate.get("finishMessage").and_then(|m| m.as_str()) {
                        if let Some((name, args)) = parse_gemini_malformed_call(msg) {
                            on_event(ProviderEvent::ToolCallComplete {
                                id: name.clone(),
                                name,
                                args,
                                thought_signature: None,
                            });
                        }
                    }
                }
            }
        }

        on_event(ProviderEvent::Done);
        Ok(())
    }
}
