//! Agentic Loop Engine
//!
//! Implements the complete execution specification from `docs/agent/Orchestrator.md`:
//! - Situations A & B: Normal completion & single tool execution.
//! - Situation C: Multi-tool batching.
//! - Situation D: Non-crashing failure feedback (tools feed errors back as data).
//! - Situation E: Unknown tool handling with registered tool names feedback.
//! - Situation G: Bounded retries for provider network errors.
//! - Situation H & I: Destructive action warning previews & rejection feedback.
//! - Situation J: Mid-execution clean interruption checks.
//! - Situation K: Loop iteration capping.
//! - Situation L: Context window overflow pruning.
//! - Situation O: Session export/import for persistence across restarts.

use std::sync::atomic::Ordering;
use serde_json::Value;

use crate::orchestrator::config::AgentConfig;
use crate::context::pruner::ContextManager;
use crate::orchestrator::session::AgentSession;
use crate::tools::registry::ToolRegistry;
use crate::traits::provider::Provider;
use crate::traits::tool::ToolRisk;
use crate::types::approval::{ApprovalDecision, ApprovalRequest};
use crate::types::error::ProviderError;
use crate::types::event::{AgentEvent, ProviderEvent};
use crate::types::message::{Message, ToolCall};
use crate::verification::VerificationRunner;

use std::collections::HashSet;
use std::sync::Arc;
use crate::skills::SkillRegistry;
use crate::orchestrator::AgentIntent;

/// The main autonomous agent orchestrator tying Provider, Tools, and Conversation together.
pub struct Agent {
    provider: Box<dyn Provider>,
    tools: ToolRegistry,
    conversation: Vec<Message>,
    config: AgentConfig,
    skills: Arc<SkillRegistry>,
    active_tools: HashSet<String>,
    ast: Arc<crate::ast::DynamicAstEngine>,
    explicit_intent: Option<AgentIntent>,
    explicit_tools: Option<HashSet<String>>,
    reflex: crate::orchestrator::BioReflexClient,
}

impl Agent {
    /// Create a new agent with a provider and tool registry.
    /// Automatically discovers built-in and workspace skills.
    pub fn new(provider: Box<dyn Provider>, tools: ToolRegistry) -> Self {
        let cwd = std::env::current_dir().unwrap_or_default();
        Self::with_workspace(provider, tools, cwd)
    }

    /// Create an agent without a workspace root (General Chat mode).
    /// Safe: does not bind to any directory, does not register filesystem/terminal tools.
    pub fn without_workspace(provider: Box<dyn Provider>, tools: ToolRegistry) -> Self {
        let skills = Arc::new(crate::skills::SkillRegistry::default());
        let ast = Arc::new(crate::ast::DynamicAstEngine::new());
        Self {
            provider,
            tools,
            conversation: Vec::new(),
            config: AgentConfig::default(),
            skills,
            active_tools: HashSet::new(),
            ast,
            explicit_intent: None,
            explicit_tools: None,
            reflex: crate::orchestrator::BioReflexClient::default(),
        }
    }

    /// Create an agent initialized for a specific workspace root, discovering project skills.
    pub fn with_workspace(
        provider: Box<dyn Provider>,
        mut tools: ToolRegistry,
        workspace: impl AsRef<std::path::Path>,
    ) -> Self {
        let ws_path = workspace.as_ref().to_path_buf();
        let skills_report = crate::skills::discover(&ws_path).unwrap_or_default();
        let skills = Arc::new(skills_report.registry);
        let ast = Arc::new(crate::ast::DynamicAstEngine::new());
        tools.register(crate::tools::ActivateSkillTool::new(skills.clone()));
        let tools_list = tools.manifest_pairs();
        let tools_provider: crate::tools::search::ToolListProvider = Arc::new(move || tools_list.clone());
        if tools.find("search").is_some() {
            tools.register(
                crate::tools::SearchTool::new()
                    .with_ast(ast.clone())
                    .with_skills(skills.clone())
                    .with_tools_provider(tools_provider),
            );
        }
        if tools.find("read_file").is_some() {
            tools.register(crate::tools::ReadFileTool::new().with_ast(ast.clone()).with_workspace(ws_path.clone()));
        }
        if tools.find("list_directory").is_some() {
            tools.register(crate::tools::ListDirectoryTool::new().with_workspace(ws_path.clone()));
        }
        if tools.find("run_terminal").is_some() {
            tools.register(crate::tools::RunTerminalTool::new().with_workspace(ws_path.clone()));
        }
        Self {
            provider,
            tools,
            conversation: Vec::new(),
            config: AgentConfig::default(),
            skills,
            active_tools: HashSet::new(),
            ast,
            explicit_intent: None,
            explicit_tools: None,
            reflex: crate::orchestrator::BioReflexClient::default(),
        }
    }

    /// Set a custom bio-reflex client (e.g. for custom endpoint or mock tests).
    pub fn with_reflex(mut self, reflex: crate::orchestrator::BioReflexClient) -> Self {
        self.reflex = reflex;
        self
    }

    /// Read-only reference to the bio-reflex client.
    pub fn reflex(&self) -> &crate::orchestrator::BioReflexClient {
        &self.reflex
    }

    /// Read-only reference to the dynamic AST engine.
    pub fn ast(&self) -> &Arc<crate::ast::DynamicAstEngine> {
        &self.ast
    }

    /// Dynamically register a Tree-sitter language grammar for an extension.
    pub fn register_language(&self, ext: &str, language: tree_sitter::Language) {
        self.ast.register_language(ext, language);
    }

    /// Dynamically register a Tree-sitter language grammar for multiple extensions.
    pub fn register_extensions(&self, exts: &[&str], language: tree_sitter::Language) {
        self.ast.register_extensions(exts, language);
    }

    /// Set or override the active intent for the next run.
    pub fn set_intent(&mut self, intent: AgentIntent) {
        self.explicit_intent = Some(intent.clone());
        self.explicit_tools = None;
        self.active_tools.clear();
        match intent {
            AgentIntent::Discussion | AgentIntent::Exploration => {
                for name in self.tools.safe_names() {
                    self.active_tools.insert(name.to_string());
                }
            }
            AgentIntent::Action(_) => {
                for name in self.tools.list_names() {
                    self.active_tools.insert(name.to_string());
                }
            }
        }


    }

    /// Add a tool to the session cache dynamically (e.g. via discovery).
    pub fn enable_tool(&mut self, tool_name: &str) {
        self.active_tools.insert(tool_name.to_string());
        self.explicit_tools.get_or_insert_with(HashSet::new).insert(tool_name.to_string());
    }

    /// Enable all tools (fallback mode).
    pub fn enable_all_tools(&mut self) {
        self.active_tools.clear();
        for name in self.tools.list_names() {
            self.active_tools.insert(name.to_string());
        }
        self.explicit_tools = Some(self.active_tools.clone());
    }

    /// Read-only slice of active tool names in the session cache.
    pub fn active_tools(&self) -> &HashSet<String> {
        &self.active_tools
    }

    /// Read-only reference to the discovered skill registry.
    pub fn skills(&self) -> &SkillRegistry {
        &self.skills
    }

    /// Set a custom configuration for this agent.
    pub fn with_config(mut self, config: AgentConfig) -> Self {
        self.config = config;
        self
    }

    /// Attach an atomic cancellation token to enable clean user interrupts mid-task.
    pub fn with_cancellation_token(mut self, token: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        self.config.cancellation_token = Some(token);
        self
    }

    /// Check if a cancellation request has been triggered.
    pub fn is_cancelled(&self) -> bool {
        self.config
            .cancellation_token
            .as_ref()
            .map(|t| t.load(Ordering::Relaxed))
            .unwrap_or(false)
    }

    /// Read-only slice of conversation history.
    pub fn conversation(&self) -> &[Message] {
        &self.conversation
    }

    /// Clear conversation history.
    pub fn clear_conversation(&mut self) {
        self.conversation.clear();
    }

    /// Prepend a system instruction message to the conversation.
    /// Call this before the first `run()` on a fresh session to inject
    /// workspace context (e.g. current directory, IDE info, available tools hint).
    pub fn push_system_message(&mut self, content: impl Into<String>) {
        use crate::types::message::Message;
        self.conversation.insert(0, Message::system(content));
    }

    /// Reference to tool registry.
    pub fn tools(&self) -> &ToolRegistry {
        &self.tools
    }

    /// Mutable reference to tool registry.
    pub fn tools_mut(&mut self) -> &mut ToolRegistry {
        &mut self.tools
    }

    /// Export session history to JSON string for persistence across IDE restarts.
    pub fn export_session_json(&self) -> Result<String, serde_json::Error> {
        let session = AgentSession {
            session_id: "default".to_string(),
            conversation: self.conversation.clone(),
        };
        session.to_json()
    }

    /// Import session history from JSON string to resume an existing session.
    pub fn import_session_json(&mut self, json_str: &str) -> Result<(), serde_json::Error> {
        let session = AgentSession::from_json(json_str)?;
        self.conversation = session.conversation;
        Ok(())
    }

    /// Execute a user request through the agent loop.
    pub fn run(
        &mut self,
        user_input: &str,
        on_event: &mut dyn FnMut(AgentEvent),
        ask_approval: &mut dyn FnMut(ApprovalRequest) -> ApprovalDecision,
    ) -> Result<String, ProviderError> {
        self.conversation.push(Message::user(user_input));

        // Dynamically provision tool schemas per turn: LLM decides autonomously unless explicitly overridden
        if let Some(tools) = self.explicit_tools.take() {
            self.active_tools = tools;
        } else if let Some(intent) = self.explicit_intent.take() {
            self.active_tools.clear();
            match intent {
                AgentIntent::Discussion | AgentIntent::Exploration => {
                    for name in self.tools.safe_names() {
                        self.active_tools.insert(name.to_string());
                    }
                }
                AgentIntent::Action(_) => {
                    for name in self.tools.list_names() {
                        self.active_tools.insert(name.to_string());
                    }
                }
            }
        } else {


            // Biological Connectome Reflex Engine: Classify user intent & enforce physical safety lockout
            if let Some(decision) = self.reflex.classify(user_input) {
                if decision.is_danger {
                    let lock_msg = format!(
                        "🚨 PHYSICAL SAFETY REFLEX LOCK ENGAGED ({} µs latency, {:.1}% confidence)\n{}\nExecution blocked to prevent workspace damage.",
                        decision.reaction_time_us, decision.confidence, decision.explanation
                    );
                    on_event(AgentEvent::Failed(lock_msg.clone()));
                    return Ok(lock_msg);
                }
                self.active_tools.clear();
                match decision.intent {
                    AgentIntent::Discussion | AgentIntent::Exploration => {
                        // Safe read-only inspection mode: dynamically provision all ReadOnly tools
                        for name in self.tools.safe_names() {
                            self.active_tools.insert(name.to_string());
                        }
                    }
                    AgentIntent::Action(_) => {
                        // Execution / mutation mode: dynamically provision ALL registered tools (zero whitelist)
                        for name in self.tools.list_names() {
                            self.active_tools.insert(name.to_string());
                        }
                    }
                }


            } else {
                // Fallback (when bio-reflex engine offline): provision all tools registered in self.tools
                self.active_tools.clear();
                for name in self.tools.list_names() {
                    self.active_tools.insert(name.to_string());
                }
            }
        }

        let mut last_call_signature = String::new();
        let mut consecutive_repeat_count = 0;

        for iteration in 0..self.config.max_iterations {
            // Adaptive Throttling: Inject a 3-second delay between consecutive iterations
            // to prevent bursting and hitting API rate limits (e.g., Gemini's 15 RPM).
            if iteration > 0 {
                std::thread::sleep(std::time::Duration::from_secs(3));
            }

            // Situation J: Check for user cancellation at safe checkpoint
            if self.is_cancelled() {
                let cancel_msg = "Agent execution was cancelled by the user.".to_string();
                on_event(AgentEvent::Failed(cancel_msg.clone()));
                return Ok(cancel_msg);
            }

            // Situation L: Context window overflow management
            ContextManager::prune_history(&mut self.conversation, self.config.max_history_messages);
            ContextManager::compact_prior_tool_results(&mut self.conversation);

            let mut assistant_text = String::new();
            let mut pending_tool_calls: Vec<ToolCall> = Vec::new();

            // On-demand tool schemas: 0 in discussion mode, 3-5 in action/explore modes
            let tool_schemas = if self.active_tools.is_empty() {
                Vec::new()
            } else {
                let active_refs: Vec<&str> = self.active_tools.iter().map(|s| s.as_str()).collect();
                self.tools.openai_schemas_for(&active_refs)
            };

            // Situation G: Stream response with bounded network retries and 413 auto-recovery
            let mut retries = 0;
            loop {
                assistant_text.clear();
                pending_tool_calls.clear();
                let mut provider_error: Option<ProviderError> = None;

                let stream_res = self.provider.complete_stream(
                    &self.conversation,
                    &tool_schemas,
                    &mut |event| {
                        if self.is_cancelled() {
                            return;
                        }
                        match event {
                            ProviderEvent::TextDelta(delta) => {
                                assistant_text.push_str(&delta);
                                on_event(AgentEvent::Text(delta));
                            }
                            ProviderEvent::ToolCallComplete {
                                id,
                                name,
                                args,
                                thought_signature,
                            } => {
                                pending_tool_calls.push(ToolCall {
                                    id,
                                    name,
                                    args,
                                    thought_signature,
                                });
                            }
                            ProviderEvent::Error(err) => {
                                provider_error = Some(ProviderError::Malformed(err));
                            }
                            _ => {}
                        }
                    },
                );

                if self.is_cancelled() {
                    let cancel_msg = "Agent execution was cancelled by the user.".to_string();
                    on_event(AgentEvent::Failed(cancel_msg.clone()));
                    return Ok(cancel_msg);
                }

                let active_err = provider_error.clone().or_else(|| stream_res.as_ref().err().cloned());
                if let Some(err) = active_err {
                    let err_str = err.to_string();
                    let is_payload_too_large = err_str.contains("413")
                        || err_str.contains("Payload Too Large")
                        || err_str.contains("Request too large")
                        || err_str.contains("rate_limit_exceeded");

                    if is_payload_too_large && retries < self.config.max_provider_retries + 1 {
                        retries += 1;
                        // Emergency compact conversation down to essentials and retry immediately
                        ContextManager::emergency_prune(&mut self.conversation);
                        continue;
                    }

                    if retries < self.config.max_provider_retries {
                        retries += 1;
                        std::thread::sleep(std::time::Duration::from_millis(200 * retries as u64));
                        continue;
                    }
                    on_event(AgentEvent::Failed(err.to_string()));
                    return Err(err);
                }

                break;
            }

            // Situation A: Plain text output with no tool calls -> Done!
            if pending_tool_calls.is_empty() {
                self.conversation.push(Message::assistant(&assistant_text));

                // ── Verification step ────────────────────────────────────────
                // If a verification command is configured, run it now.
                // On failure: inject the error back as a new user turn so the
                // agent can fix the issue and the loop continues.
                if let Some(ref cmd) = self.config.verification_command.clone() {
                    let cwd = std::env::current_dir()
                        .ok()
                        .and_then(|p| p.to_str().map(|s| s.to_string()));

                    on_event(AgentEvent::Text(format!("\n🔍 Verifying: `{}`\n", cmd)));

                    let result = VerificationRunner::run(cmd, cwd.as_deref());
                    if result.is_pass() {
                        on_event(AgentEvent::Finished);
                        return Ok(assistant_text);
                    } else {
                        // Feed failure back into the loop as a new user message
                        let feedback = result.as_feedback(cmd);
                        on_event(AgentEvent::Text(format!("\n⚠️ Verification failed, asking agent to fix...\n")));
                        self.conversation.push(Message::user(&feedback));
                        // Continue the outer for loop — agent gets another turn
                        continue;
                    }
                }
                // ── End verification ─────────────────────────────────────────

                on_event(AgentEvent::Finished);
                return Ok(assistant_text);
            }

            // Record assistant turn with tool calls in history
            self.conversation.push(Message::assistant_with_tools(
                &assistant_text,
                pending_tool_calls.clone(),
            ));

            // Situation B & C: Process tool calls (batch gathered for next LLM turn)
            for tool_call in pending_tool_calls {
                // Check cancellation between tool steps
                if self.is_cancelled() {
                    let cancel_msg = "Task interrupted by user. Stopping cleanly.";
                    on_event(AgentEvent::Failed(cancel_msg.to_string()));
                    self.conversation.push(Message::tool_result(&tool_call.id, cancel_msg));
                    return Ok(cancel_msg.to_string());
                }

                let tool_name = &tool_call.name;
                let tool_id = &tool_call.id;
                let mut tool_args = tool_call.args;

                // Repetitive call loop breaker: Prevents model from getting stuck repeating identical tool calls
                let call_sig = format!("{}:{}", tool_name, tool_args);
                if call_sig == last_call_signature {
                    consecutive_repeat_count += 1;
                } else {
                    consecutive_repeat_count = 1;
                    last_call_signature = call_sig.clone();
                }

                if consecutive_repeat_count >= 3 {
                    let loop_msg = format!(
                        "Repetitive call blocked: You have called tool '{}' with identical arguments {} times consecutively. \
                         You already have this output in your context. \
                         Do NOT call this tool again. Immediately formulate your final response or conclusion for the user.",
                        tool_name, consecutive_repeat_count
                    );
                    self.conversation.push(Message::tool_result(tool_id, &loop_msg));
                    continue;
                }

                // Situation E: Model requested unknown / hallucinated tool
                let tool = match self.tools.find(tool_name) {
                    Some(t) => {
                        self.active_tools.insert(tool_name.clone());
                        t
                    }
                    None => {
                        let valid = self.tools.list_names().join(", ");
                        let err_msg = format!(
                            "Error: Unknown tool '{}'. Available tools are: [{}]. Please call a valid tool.",
                            tool_name, valid
                        );
                        self.conversation.push(Message::tool_result(tool_id, &err_msg));
                        continue;
                    }
                };

                // Situation H: Destructive action gating & preview rendering
                if !self.config.auto_approve_all && tool.risk() != ToolRisk::ReadOnly {
                    let preview = match tool.risk() {
                        ToolRisk::Destructive => {
                            format!("⚠️ DESTRUCTIVE ACTION: '{}'\nArguments: {}", tool_name, tool_args)
                        }
                        _ => format!("Tool '{}': {}", tool_name, tool_args),
                    };

                    let req = ApprovalRequest {
                        id: tool_id.clone(),
                        tool_name: tool_name.clone(),
                        args: tool_args.clone(),
                        preview,
                    };
                    on_event(AgentEvent::NeedsApproval(req.clone()));

                    match ask_approval(req) {
                        ApprovalDecision::Approve => {}
                        ApprovalDecision::ApproveWithEdits(new_args) => {
                            tool_args = new_args;
                        }
                        // Situation I: User rejects a proposed action
                        ApprovalDecision::Reject => {
                            let reject_msg = format!("User rejected execution of tool '{}'. Please try an alternative approach.", tool_name);
                            self.conversation.push(Message::tool_result(tool_id, &reject_msg));
                            continue;
                        }
                    }
                }

                on_event(AgentEvent::ToolStarted {
                    name: tool_name.clone(),
                    args: tool_args.clone(),
                });

                // Situation D: Tool execution (failures feed back to model as data, never crash loop)
                match tool.execute(tool_args) {
                    Ok(result) => {
                        on_event(AgentEvent::ToolFinished {
                            name: tool_name.clone(),
                            result: result.clone(),
                        });
                        let result_str = match &result {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        let sanitized_result = ContextManager::sanitize_tool_result(&result_str, self.config.max_tool_output_chars);
                        self.conversation.push(Message::tool_result(tool_id, sanitized_result));

                        // Dynamic mid-task tool discovery: If `search` discovered tools (in: "tools" or "tools+skills"),
                        // elevate those tool names into `self.active_tools` (the session cache)
                        // so their full schemas are automatically provisioned in the next turn of the loop.
                        if tool_name == "search" && result_str.contains("[tools]") {
                            let mut in_tools_section = false;
                            for line in result_str.lines() {
                                let trimmed = line.trim();
                                if trimmed == "[tools]" {
                                    in_tools_section = true;
                                    continue;
                                } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
                                    in_tools_section = false;
                                }
                                if in_tools_section {
                                    if let Some((cand, _)) = trimmed.split_once(':') {
                                        let cand = cand.trim();
                                        if self.tools.find(cand).is_some() {
                                            self.active_tools.insert(cand.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let err_str = format!("Tool '{}' failed: {}", tool_name, e);
                        let sanitized_err = ContextManager::sanitize_tool_result(&err_str, 2000);
                        self.conversation.push(Message::tool_result(tool_id, sanitized_err));
                    }
                }
            }

            // Loop continues: all tool results are now in conversation; sent back in next turn
        }

        // Situation K: Loop reached maximum iterations cap
        let cap_msg = format!(
            "Agent reached maximum iterations ({}) without concluding.",
            self.config.max_iterations
        );
        on_event(AgentEvent::Failed(cap_msg.clone()));
        Ok(cap_msg)
    }
}
