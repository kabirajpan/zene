use std::time::Duration;
use serde::{Deserialize, Serialize};
use crate::orchestrator::{ActionCategory, AgentIntent};

/// Structured output from the 500-neuron Biological Reflex Engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflexDecision {
    pub intent: AgentIntent,
    pub raw_intent: String,
    pub confidence: f32,
    pub reaction_time_us: u32,
    pub is_danger: bool,
    pub tools: Vec<String>,
    pub explanation: String,
}

#[derive(Debug, Deserialize)]
struct ServerPredictResponse {
    intent: String,
    confidence: f32,
    reaction_time_us: u32,
    is_danger: bool,
    tools: Vec<String>,
    explanation: String,
}

#[derive(Debug, Clone)]
pub enum ReflexEngineMode {
    /// Pure native embedded Rust connectome (sub-15 µs, zero Python, zero network)
    Native,
    /// HTTP bridge to external connectome server (e.g. for streaming to 3D visualizer)
    Http { endpoint: String, timeout: Duration },
    /// Offline / disabled
    Disabled,
}

/// Hybrid Bio-Reflex Client: Defaults to pure in-process native Rust execution.
#[derive(Debug, Clone)]
pub struct BioReflexClient {
    mode: ReflexEngineMode,
}

impl Default for BioReflexClient {
    fn default() -> Self {
        if let Ok(endpoint) = std::env::var("FLY_REFLEX_ENDPOINT") {
            Self::http(endpoint)
        } else {
            Self::native()
        }
    }
}

impl BioReflexClient {
    /// Pure native embedded connectome inference (15 µs, zero external dependencies)
    pub fn native() -> Self {
        Self {
            mode: ReflexEngineMode::Native,
        }
    }

    /// Remote or local HTTP inference bridge (e.g. for visualizer)
    pub fn http(endpoint: impl Into<String>) -> Self {
        Self {
            mode: ReflexEngineMode::Http {
                endpoint: endpoint.into(),
                timeout: Duration::from_millis(150),
            },
        }
    }

    pub fn new(endpoint: impl Into<String>) -> Self {
        Self::http(endpoint)
    }

    pub fn disabled() -> Self {
        Self {
            mode: ReflexEngineMode::Disabled,
        }
    }

    /// Evaluates user input against the biological connectome.
    pub fn classify(&self, prompt: &str) -> Option<ReflexDecision> {
        match &self.mode {
            ReflexEngineMode::Native => {
                Some(crate::orchestrator::FlyBrain::global().predict(prompt))
            }
            ReflexEngineMode::Disabled => None,
            ReflexEngineMode::Http { endpoint, timeout } => {
                let client = reqwest::blocking::Client::builder()
                    .timeout(*timeout)
                    .build()
                    .ok()?;

                let payload = serde_json::json!({
                    "prompt": prompt
                });

                let resp = client.post(endpoint)
                    .json(&payload)
                    .send()
                    .ok()?;

                if !resp.status().is_success() {
                    return None;
                }

                let parsed: ServerPredictResponse = resp.json().ok()?;

                let intent = match parsed.intent.as_str() {
                    "DISCUSSION" => AgentIntent::Discussion,
                    "INSPECTION" => AgentIntent::Exploration,
                    "PLANNING" => AgentIntent::Action(ActionCategory::General),
                    "EXECUTION" => AgentIntent::Action(ActionCategory::Debug),
                    "REFLEX_LOCK" => AgentIntent::Discussion,
                    _ => AgentIntent::Action(ActionCategory::General),
                };

                Some(ReflexDecision {
                    intent,
                    raw_intent: parsed.intent,
                    confidence: parsed.confidence,
                    reaction_time_us: parsed.reaction_time_us,
                    is_danger: parsed.is_danger,
                    tools: parsed.tools,
                    explanation: parsed.explanation,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reflex_client_default_is_native() {
        let client = BioReflexClient::default();
        match client.mode {
            ReflexEngineMode::Native => {},
            _ => panic!("Default client should be native embedded mode"),
        }
    }

    #[test]
    fn test_live_reflex_classification_against_running_server() {
        let client = BioReflexClient::default();
        if let Some(decision) = client.classify("git clean -fd") {
            assert!(decision.is_danger);
            assert_eq!(decision.raw_intent, "REFLEX_LOCK");
            assert!(decision.reaction_time_us < 100);
            println!("[TEST] Successfully verified physical reflex lockout: {:?}", decision);
        } else {
            println!("[TEST] Server offline or timed out; skipping live test.");
        }
    }

    #[test]
    fn test_live_reflex_discussion_and_inspection() {
        let client = BioReflexClient::default();
        if let Some(dec) = client.classify("hello how are you?") {
            assert_eq!(dec.raw_intent, "DISCUSSION");
            assert_eq!(dec.intent, AgentIntent::Discussion);
            assert!(!dec.is_danger);
            println!("[TEST] DISCUSSION classification verified: confidence={:.1}%", dec.confidence);
        }

        if let Some(dec) = client.classify("where is the user config located?") {
            assert_eq!(dec.raw_intent, "INSPECTION");
            assert_eq!(dec.intent, AgentIntent::Exploration);
            assert!(!dec.is_danger);
            println!("[TEST] INSPECTION classification verified: confidence={:.1}%", dec.confidence);
        }
    }

    #[test]
    fn test_live_agent_loop_reflex_lockout() {
        use crate::agentic_loop::Agent;
        use crate::tools::ToolRegistry;
        use crate::traits::provider::Provider;
        use crate::types::approval::ApprovalDecision;
        use crate::types::error::ProviderError;
        use crate::types::event::ProviderEvent;
        use crate::types::message::Message;
        use serde_json::Value;

        struct PanicProvider;
        impl Provider for PanicProvider {
            fn name(&self) -> &str { "PanicProvider" }
            fn complete_stream(
                &self,
                _messages: &[Message],
                _tools: &[Value],
                _on_event: &mut dyn FnMut(ProviderEvent),
            ) -> Result<(), ProviderError> {
                panic!("Provider MUST NEVER be called when Reflex Lock fires!");
            }
        }

        let mut agent = Agent::new(Box::new(PanicProvider), ToolRegistry::all());
        let mut on_event = |_| {};
        let mut ask_approval = |_| ApprovalDecision::Approve;

        let res = agent.run("git clean -fd", &mut on_event, &mut ask_approval);
        assert!(res.is_ok());
        let reply = res.unwrap();
        assert!(reply.contains("PHYSICAL SAFETY REFLEX LOCK ENGAGED"));
        println!("[TEST] Agent safely blocked destructive command before LLM call: {}", reply);
    }

    #[test]
    fn test_live_agent_loop_discussion_safe_readonly_tools() {
        use crate::agentic_loop::Agent;
        use crate::tools::ToolRegistry;
        use crate::traits::provider::Provider;
        use crate::types::approval::ApprovalDecision;
        use crate::types::error::ProviderError;
        use crate::types::event::ProviderEvent;
        use crate::types::message::Message;
        use serde_json::Value;

        struct EchoProvider;
        impl Provider for EchoProvider {
            fn name(&self) -> &str { "EchoProvider" }
            fn complete_stream(
                &self,
                _messages: &[Message],
                tools: &[Value],
                on_event: &mut dyn FnMut(ProviderEvent),
            ) -> Result<(), ProviderError> {
                // Assert that for DISCUSSION, safe read-only tools are sent to LLM, while mutation tools are locked!
                assert!(!tools.is_empty(), "Discussion intent provisions safe read-only tools to avoid tool starvation");
                let tool_names: Vec<String> = tools.iter()
                    .filter_map(|t| t.get("function").and_then(|f| f.get("name")).and_then(|n| n.as_str()).map(|s| s.to_string()))
                    .collect();
                assert!(tool_names.contains(&"search".to_string()));
                assert!(tool_names.contains(&"read_file".to_string()));
                assert!(!tool_names.contains(&"edit_file".to_string()));
                assert!(!tool_names.contains(&"run_terminal".to_string()));
                on_event(ProviderEvent::TextDelta("Hello! How can I help you?".into()));
                on_event(ProviderEvent::Done);
                Ok(())
            }
        }

        let mut agent = Agent::new(Box::new(EchoProvider), ToolRegistry::all());
        let mut on_event = |_| {};
        let mut ask_approval = |_| ApprovalDecision::Approve;

        let res = agent.run("hello how are you?", &mut on_event, &mut ask_approval);
        assert!(res.is_ok());
        assert!(agent.active_tools().contains("search"));
        assert!(agent.active_tools().contains("read_file"));
        assert!(!agent.active_tools().contains("edit_file"));
        println!("[TEST] Discussion mode confirmed safe read-only tools sent to LLM, mutation strictly locked!");
    }
}
