pub mod agentic_loop;
pub mod ast;
pub mod context;
pub mod orchestrator;
pub mod provider;
pub mod skills;
pub mod tools;
pub mod traits;
pub mod types;
pub mod verification;

pub use agentic_loop::Agent;
pub use orchestrator::AgentConfig;
pub use tools::ToolRegistry;


/// Constructs an `Agent` initialized with the default provider and the full essential toolset.
pub fn create_agent() -> Option<Agent> {
    let provider = provider::create_default_provider()?;
    let tools = ToolRegistry::essentials();
    Some(Agent::new(provider, tools))
}

/// Constructs an `Agent` for a specific provider + model ID.
/// `provider_name` is `"gemini"` or `"groq"` (case-insensitive).
/// `model_id` is the raw model string (e.g. `"gemini-2.5-flash"`, `"qwen/qwen3-32b"`).
pub fn create_agent_for_model(provider_name: &str, model_id: &str) -> Option<Agent> {
    let provider = provider::create_provider_for(provider_name, model_id)?;
    let tools = ToolRegistry::essentials();
    Some(Agent::new(provider, tools))
}

/// Constructs an `Agent` for a specific provider + model ID and workspace root.
pub fn create_agent_for_model_and_workspace(
    provider_name: &str,
    model_id: &str,
    workspace: impl AsRef<std::path::Path>,
) -> Option<Agent> {
    let provider = provider::create_provider_for(provider_name, model_id)?;
    let tools = ToolRegistry::essentials();
    Some(Agent::with_workspace(provider, tools, workspace))
}

/// Constructs an `Agent` for General Chat mode with NO workspace attached.
/// Has no filesystem mutating tools or terminal tools.
pub fn create_agent_without_workspace(provider_name: &str, model_id: &str) -> Option<Agent> {
    let provider = provider::create_provider_for(provider_name, model_id)?;
    let tools = ToolRegistry::new();
    Some(Agent::without_workspace(provider, tools))
}

/// Constructs an `Agent` initialized with ONLY the 2 read-only tools:
/// `read_file` and `get_diagnostics`.
///
/// Both are strictly read-only with no approval gate needed, perfect for proving
/// the loop end-to-end before adding approval UI and additional providers.
pub fn create_minimal_agent() -> Option<Agent> {
    let provider = provider::create_default_provider()?;
    let tools = ToolRegistry::minimal();
    Some(Agent::new(provider, tools))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires live API credentials & quota"]
    fn test_provider_completion() {
        let res = provider::complete("Say 'agent online' and nothing else.");
        assert!(res.is_ok(), "Failed completion: {:?}", res.err());
        let reply = res.unwrap();
        println!("Provider response: {}", reply);
        assert!(!reply.is_empty());
    }

    #[test]
    #[ignore = "requires live API credentials & quota"]
    fn test_create_agent_end_to_end() {
        if let Some(mut agent) = create_agent() {
            let mut events = Vec::new();
            let mut on_event = |ev| events.push(ev);
            let mut ask_approval = |_req| types::ApprovalDecision::Approve;

            let res = agent.run("Reply 'Loop engine ready.'", &mut on_event, &mut ask_approval);
            if let Err(e) = &res {
                println!("Agent run error: {:?}", e);
            }
            assert!(res.is_ok());
            println!("Agent response: {:?}", res.unwrap());
            assert!(!events.is_empty());
        }
    }

    #[test]
    #[ignore = "requires live API credentials & quota"]
    fn test_two_readonly_tools_loop_proof() {
        if let Some(mut agent) = create_minimal_agent() {
            // Verify registry contains strictly the 3 read-only tools
            assert_eq!(agent.tools().len(), 3);
            assert!(agent.tools().find("read_file").is_some());
            assert!(agent.tools().find("get_diagnostics").is_some());
            assert!(agent.tools().find("activate_skill").is_some());

            let mut tools_executed = Vec::new();
            let mut on_event = |ev| {
                if let types::AgentEvent::ToolStarted { name, .. } = &ev {
                    println!("[Agent executing tool: {}]", name);
                    tools_executed.push(name.clone());
                }
            };
            let mut ask_approval = |_req| types::ApprovalDecision::Approve;

            // Prompt specifically asking model to inspect project diagnostics
            let prompt = "Please check the current compiler diagnostics using get_diagnostics and summarize if there are any errors.";
            let res = agent.run(prompt, &mut on_event, &mut ask_approval);
            assert!(res.is_ok(), "Agent run failed: {:?}", res.err());
            let final_reply = res.unwrap();
            println!("Final model answer: {}", final_reply);

            // Verify conversation history has:
            // 1. User prompt
            // 2. Assistant turn with tool_calls
            // 3. Tool result turn with diagnostic output
            // 4. Final Assistant answer
            let conv = agent.conversation();
            assert!(conv.len() >= 3, "Expected multi-turn conversation, got len: {}", conv.len());
            assert_eq!(conv[0].role, types::Role::User);
            assert_eq!(conv[1].role, types::Role::Assistant);
            assert!(conv[1].tool_calls.is_some(), "Expected assistant to invoke tool");
            assert_eq!(conv[2].role, types::Role::Tool);
            println!("Proved: multi-turn loop successfully executed tool and fed result back!");
        }
    }
}
