pub mod brain;
pub mod config;
pub mod intent;
pub mod reflex;
pub mod session;

pub use brain::FlyBrain;
pub use config::AgentConfig;
pub use intent::{ActionCategory, AgentIntent};
pub use reflex::{BioReflexClient, ReflexDecision};
pub use session::AgentSession;


#[cfg(test)]
mod tests {
    use crate::agentic_loop::Agent;
    use crate::context::pruner::ContextManager;
    use crate::tools::registry::ToolRegistry;
    use crate::traits::provider::Provider;
    use crate::types::approval::{ApprovalDecision, ApprovalRequest};
    use crate::types::error::ProviderError;
    use crate::types::event::{AgentEvent, ProviderEvent};
    use crate::types::message::{Message, Role};
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use crate::orchestrator::{ActionCategory, AgentIntent};

    /// Mock provider simulating a multi-turn tool calling loop
    struct MockEchoProvider {
        turn: AtomicUsize,
    }

    impl MockEchoProvider {
        fn new() -> Self {
            Self {
                turn: AtomicUsize::new(0),
            }
        }
    }

    impl Provider for MockEchoProvider {
        fn name(&self) -> &str {
            "MockEchoProvider"
        }

        fn complete_stream(
            &self,
            messages: &[Message],
            _tools: &[Value],
            on_event: &mut dyn FnMut(ProviderEvent),
        ) -> Result<(), ProviderError> {
            let current = self.turn.fetch_add(1, Ordering::SeqCst);

            if current == 0 {
                on_event(ProviderEvent::TextDelta("Checking Cargo.toml... ".into()));
                on_event(ProviderEvent::ToolCallComplete {
                    id: "call_123".into(),
                    name: "read_file".into(),
                    args: json!({ "path": "Cargo.toml", "start_line": 1, "end_line": 2 }),
                    thought_signature: None,
                });
                on_event(ProviderEvent::Done);
            } else {
                let has_tool_result = messages.iter().any(|m| m.role == Role::Tool);
                assert!(has_tool_result, "Expected conversation to contain tool result");

                on_event(ProviderEvent::TextDelta("File read successfully: [package]".into()));
                on_event(ProviderEvent::Done);
            }

            Ok(())
        }
    }

    #[test]
    fn test_orchestrator_multi_turn_loop() {
        let provider = Box::new(MockEchoProvider::new());
        let tools = ToolRegistry::minimal();
        let mut agent = Agent::new(provider, tools);

        let mut events_captured = Vec::new();
        let mut on_event = |event: AgentEvent| {
            events_captured.push(event);
        };
        let mut ask_approval = |_req: ApprovalRequest| ApprovalDecision::Approve;

        let result = agent.run("Please check Cargo.toml", &mut on_event, &mut ask_approval);
        assert!(result.is_ok());
        let final_text = result.unwrap();
        assert!(final_text.contains("File read successfully"));

        let history = agent.conversation();
        assert_eq!(history.len(), 4);
        assert_eq!(history[0].role, Role::User);
        assert_eq!(history[1].role, Role::Assistant);
        assert!(history[1].tool_calls.is_some());
        assert_eq!(history[2].role, Role::Tool);
        assert_eq!(history[3].role, Role::Assistant);
    }

    #[test]
    fn test_session_persistence() {
        let provider = Box::new(MockEchoProvider::new());
        let tools = ToolRegistry::minimal();
        let mut agent = Agent::new(provider, tools);

        let mut on_event = |_| {};
        let mut ask_approval = |_| ApprovalDecision::Approve;

        let _ = agent.run("Hello", &mut on_event, &mut ask_approval);
        assert!(!agent.conversation().is_empty());

        // Export to JSON
        let json_data = agent.export_session_json().expect("Session export failed");
        assert!(json_data.contains("Hello"));

        // Import into clean agent
        let provider2 = Box::new(MockEchoProvider::new());
        let mut restored_agent = Agent::new(provider2, ToolRegistry::minimal());
        restored_agent.import_session_json(&json_data).expect("Session import failed");

        assert_eq!(restored_agent.conversation().len(), agent.conversation().len());
    }

    #[test]
    fn test_cancellation_interruption() {
        let provider = Box::new(MockEchoProvider::new());
        let tools = ToolRegistry::minimal();
        let cancel_token = Arc::new(AtomicBool::new(false));

        let mut agent = Agent::new(provider, tools)
            .with_cancellation_token(cancel_token.clone());

        // Cancel immediately
        cancel_token.store(true, Ordering::SeqCst);

        let mut on_event = |_| {};
        let mut ask_approval = |_| ApprovalDecision::Approve;

        let result = agent.run("Should stop immediately", &mut on_event, &mut ask_approval);
        assert!(result.is_ok());
        let msg = result.unwrap();
        assert!(msg.contains("cancelled"));
    }

    #[test]
    fn test_context_pruning() {
        let mut conversation = Vec::new();
        conversation.push(Message::user("Initial task prompt"));
        for i in 1..=20 {
            conversation.push(Message::assistant(format!("Turn {}", i)));
        }
        assert_eq!(conversation.len(), 21);

        ContextManager::prune_history(&mut conversation, 10);
        assert!(conversation.len() <= 10);
        // Ensure initial user task is preserved
        assert_eq!(conversation[0].content, "Initial task prompt");
    }

    #[test]
    fn test_utf8_tool_result_sanitizing() {
        // Multi-byte Unicode tree glyphs and emojis
        let heavy_output = "├── 📁 src/\n└── 🦀 main.rs\n".repeat(200);
        assert!(heavy_output.len() > 1000);

        let sanitized = ContextManager::sanitize_tool_result(&heavy_output, 200);
        assert!(sanitized.contains("[Output truncated:"));
        assert!(sanitized.contains("🦀 main.rs"));
    }

    #[test]
    fn test_dynamic_intent_tool_provisioning_per_turn() {
        struct MockTurnEcho;
        impl Provider for MockTurnEcho {
            fn name(&self) -> &str { "MockTurnEcho" }
            fn complete_stream(
                &self,
                _messages: &[Message],
                _tools: &[Value],
                on_event: &mut dyn FnMut(ProviderEvent),
            ) -> Result<(), ProviderError> {
                on_event(ProviderEvent::TextDelta("Done".into()));
                on_event(ProviderEvent::Done);
                Ok(())
            }
        }

        // Use an offline endpoint here to test the fallback behavior when reflex engine is offline
        let mut agent = Agent::new(Box::new(MockTurnEcho), ToolRegistry::all())
            .with_reflex(crate::orchestrator::BioReflexClient::new("http://127.0.0.1:9999/offline"));
        let mut on_event = |_| {};
        let mut ask_approval = |_| ApprovalDecision::Approve;

        // Turn 1: Default run -> all tools provisioned, LLM decides autonomously
        let _ = agent.run("Can you explain the difference between mutex and rwlock?", &mut on_event, &mut ask_approval);
        assert!(agent.active_tools().contains("search"));
        assert!(agent.active_tools().contains("read_file"));
        assert!(agent.active_tools().contains("edit_file"));

        // Turn 2: Explicit Discussion intent -> safe read-only active tools, mutation locked
        agent.set_intent(AgentIntent::Discussion);
        let _ = agent.run("Why is that approach better?", &mut on_event, &mut ask_approval);
        assert!(agent.active_tools().contains("search"));
        assert!(agent.active_tools().contains("read_file"));
        assert!(!agent.active_tools().contains("edit_file"));

        // Turn 3: Explicit Exploration intent -> search, read_file, activate_skill
        agent.set_intent(AgentIntent::Exploration);
        let _ = agent.run("Where is the user configuration file located?", &mut on_event, &mut ask_approval);
        assert!(agent.active_tools().contains("search"));
        assert!(agent.active_tools().contains("read_file"));
        assert!(agent.active_tools().contains("activate_skill"));
        assert!(!agent.active_tools().contains("edit_file"));

        // Turn 4: Explicit Action (Debug) intent -> edit_file, get_diagnostics, etc.
        agent.set_intent(AgentIntent::Action(ActionCategory::Debug));
        let _ = agent.run("Fix the compiler error in main.rs", &mut on_event, &mut ask_approval);
        assert!(agent.active_tools().contains("edit_file"));
        assert!(agent.active_tools().contains("get_diagnostics"));

        // Turn 5: Next turn without explicit intent resets back to full toolset for LLM choice
        let _ = agent.run("Next question", &mut on_event, &mut ask_approval);
        assert!(agent.active_tools().contains("search"));
        assert!(agent.active_tools().contains("edit_file"));
    }

    #[test]
    fn test_search_tools_elevates_into_session_cache() {
        struct SearchEchoProvider {
            turn: AtomicUsize,
        }
        impl Provider for SearchEchoProvider {
            fn name(&self) -> &str { "SearchEchoProvider" }
            fn complete_stream(
                &self,
                _messages: &[Message],
                _tools: &[Value],
                on_event: &mut dyn FnMut(ProviderEvent),
            ) -> Result<(), ProviderError> {
                let current = self.turn.fetch_add(1, Ordering::SeqCst);
                if current == 0 {
                    on_event(ProviderEvent::ToolCallComplete {
                        id: "call_search".into(),
                        name: "search".into(),
                        args: json!({ "query": "terminal", "in": "tools" }),
                        thought_signature: None,
                    });
                    on_event(ProviderEvent::Done);
                } else {
                    on_event(ProviderEvent::TextDelta("Discovered terminal tool".into()));
                    on_event(ProviderEvent::Done);
                }
                Ok(())
            }
        }

        let provider = Box::new(SearchEchoProvider { turn: AtomicUsize::new(0) });
        let mut agent = Agent::new(provider, ToolRegistry::all());
        // Set explicit intent to Exploration (so run doesn't reclassify "Explore tools" to General Action)
        agent.set_intent(AgentIntent::Exploration);
        assert!(!agent.active_tools().contains("run_terminal"));

        let mut on_event = |_| {};
        let mut ask_approval = |_| ApprovalDecision::Approve;

        let _ = agent.run("Explore tools", &mut on_event, &mut ask_approval);

        // After search returned run_terminal, it must be elevated into active_tools!
        assert!(agent.active_tools().contains("run_terminal"));
    }
}
