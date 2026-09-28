use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::traits::provider::Provider;
use crate::types::error::ProviderError;
use crate::types::event::ProviderEvent;
use crate::types::message::{Message, Role};

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Deserialize)]
struct ChatMessageResponse {
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Option<Vec<ChatChoice>>,
    error: Option<serde_json::Value>,
}

/// Simple one-shot text completion helper.
pub fn chat_completion(api_key: &str, prompt: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let req_body = ChatRequest {
        model: "qwen/qwen3.8-27b".to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        }],
    };

    let response = client
        .post("https://api.groq.com/openai/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .header("Content-Type", "application/json")
        .json(&req_body)
        .send()
        .map_err(|e| format!("Groq request failed: {e}"))?;

    let status = response.status();
    let resp_text = response
        .text()
        .map_err(|e| format!("Failed to read Groq response: {e}"))?;

    if !status.is_success() {
        return Err(format!("Groq API error (status {status}): {resp_text}"));
    }

    let parsed: ChatResponse = serde_json::from_str(&resp_text)
        .map_err(|e| format!("Failed to parse Groq response: {e}. Raw: {resp_text}"))?;

    if let Some(choices) = parsed.choices {
        if let Some(first) = choices.into_iter().next() {
            return Ok(first.message.content.trim().to_string());
        }
    }

    if let Some(err) = parsed.error {
        return Err(format!("Groq error response: {err}"));
    }

    Err("Empty response from Groq".to_string())
}

/// Groq LLM backend implementing the unified `Provider` trait.
pub struct GroqProvider {
    api_key: String,
    model: String,
}

/// Default model for the GroqProvider.
const DEFAULT_GROQ_MODEL: &str = "qwen/qwen3.8-27b";

impl GroqProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: DEFAULT_GROQ_MODEL.to_string(),
        }
    }

    pub fn with_model(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
        }
    }
}

impl Provider for GroqProvider {
    fn name(&self) -> &str {
        "Groq"
    }

    fn complete_stream(
        &self,
        messages: &[Message],
        tools: &[Value],
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<(), ProviderError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        let mut req_messages = Vec::new();
        for m in messages {
            let role_str = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
                Role::System => "system",
            };

            let mut msg_obj = serde_json::Map::new();
            msg_obj.insert("role".to_string(), json!(role_str));
            msg_obj.insert("content".to_string(), json!(m.content));

            if let Some(tool_calls) = &m.tool_calls {
                let calls_json: Vec<Value> = tool_calls
                    .iter()
                    .map(|c| {
                        json!({
                            "id": c.id,
                            "type": "function",
                            "function": {
                                "name": c.name,
                                "arguments": c.args.to_string()
                            }
                        })
                    })
                    .collect();
                msg_obj.insert("tool_calls".to_string(), Value::Array(calls_json));
            }

            if let Some(tool_call_id) = &m.tool_call_id {
                msg_obj.insert("tool_call_id".to_string(), json!(tool_call_id));
            }

            req_messages.push(Value::Object(msg_obj));
        }

        // Safeguard for models using MiniJinja chat templates (e.g. Qwen):
        // Jinja templates raise an exception if there is no message with role 'user'.
        let has_user_msg = req_messages
            .iter()
            .any(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"));
        if !has_user_msg {
            let mut fallback = serde_json::Map::new();
            fallback.insert("role".to_string(), json!("user"));
            fallback.insert(
                "content".to_string(),
                json!("Proceed with the task using the provided context."),
            );
            let insert_pos = if req_messages
                .first()
                .and_then(|m| m.get("role"))
                .and_then(|r| r.as_str())
                == Some("system")
            {
                1
            } else {
                0
            };
            req_messages.insert(insert_pos, Value::Object(fallback));
        }

        if tools.is_empty() {
            let reminder = json!({
                "role": "system",
                "content": "Notice: You are in pure conversational mode for this turn. Do not emit any tool calls or function calls; reply directly in markdown."
            });
            req_messages.push(reminder);
        }

        let mut body = json!({
            "model": self.model,
            "messages": req_messages,
        });

        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.to_vec());
        }

        // Default max_tokens to 1000 for Groq to adhere to the 1000 OTPM on-demand tier limit
        if body.get("max_tokens").is_none() {
            body["max_tokens"] = json!(1000);
        }

        // Send the request with 429 retry logic.
        // Transient spikes with small retry-after (< 10s) are retried up to 2 times.
        // Long pauses (daily quotas, TPD exhaustion) fail immediately rather than freezing the UI.
        const MAX_RETRIES: u32 = 2;
        let mut attempts = 0;
        let (status, resp_text) = loop {
            let response = client
                .post("https://api.groq.com/openai/v1/chat/completions")
                .header("Authorization", format!("Bearer {}", self.api_key.trim()))
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
                    .unwrap_or(3)
            } else {
                0
            };

            let resp_text = response
                .text()
                .map_err(|e| ProviderError::Network(e.to_string()))?;

            // If retry_secs is large (> 8s) or quota exhausted, do not freeze the UI with long sleep
            if status.as_u16() == 429 {
                let is_daily_quota = resp_text.contains("tokens per day") || resp_text.contains("TPD");
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
            eprintln!("[Groq Provider] Error: status {} | Body: {}", status, resp_text);
            // Graceful Error Recovery: If Groq returns 400 Bad Request with tool_use_failed,
            // recover the failed_generation tool call so the agentic loop proceeds without crashing.
            if status.as_u16() == 400 && (resp_text.contains("tool_use_failed") || resp_text.contains("Tool choice is none")) {
                if let Ok(err_val) = serde_json::from_str::<Value>(&resp_text) {
                    if let Some(failed_gen) = err_val.get("error").and_then(|e| e.get("failed_generation")).and_then(|fg| fg.as_str()) {
                        if let Ok(gen_obj) = serde_json::from_str::<Value>(failed_gen) {
                            let name = gen_obj.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                            let args = gen_obj.get("arguments").cloned().unwrap_or(json!({}));
                            if !name.is_empty() {
                                on_event(ProviderEvent::ToolCallComplete {
                                    id: "recovered_tool_call".to_string(),
                                    name,
                                    args,
                                    thought_signature: None,
                                });
                                on_event(ProviderEvent::Done);
                                return Ok(());
                            }
                        }
                    }
                }
            }

            return Err(ProviderError::Malformed(format!(
                "Status {}: {}",
                status, resp_text
            )));
        }

        let parsed: Value = serde_json::from_str(&resp_text)
            .map_err(|e| ProviderError::Malformed(format!("JSON parse error: {e}")))?;

        if let Some(choices) = parsed.get("choices").and_then(|c| c.as_array()) {
            if let Some(choice) = choices.first() {
                if let Some(message) = choice.get("message") {
                    if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                        if !content.is_empty() {
                            on_event(ProviderEvent::TextDelta(content.to_string()));
                        }
                    }

                    if let Some(tool_calls) = message.get("tool_calls").and_then(|tc| tc.as_array()) {
                        for tc in tool_calls {
                            let id = tc
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let func = tc.get("function");
                            let name = func
                                .and_then(|f| f.get("name"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let args_raw = func
                                .and_then(|f| f.get("arguments"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("{}");
                            let args: Value =
                                serde_json::from_str(args_raw).unwrap_or(json!({}));

                            on_event(ProviderEvent::ToolCallComplete {
                                id,
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
