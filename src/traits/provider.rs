use serde_json::Value;
use crate::types::{error::ProviderError, event::ProviderEvent, message::Message};

/// Standard interface implemented by LLM providers (Gemini, Groq, local models).
pub trait Provider: Send + Sync {
    /// Human-readable provider and model identification (e.g., "Groq (Llama-3.3-70B)").
    fn name(&self) -> &str;

    /// Stream events (text deltas, tool calls) in response to conversation messages and available tool schemas.
    fn complete_stream(
        &self,
        messages: &[Message],
        tools: &[Value],
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<(), ProviderError>;
}
