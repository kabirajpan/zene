use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use agent::types::approval::{ApprovalDecision, ApprovalRequest};
use agent::types::error::ProviderError;
use agent::types::event::{AgentEvent, ProviderEvent};
use agent::types::message::Message;
use agent::traits::provider::Provider;
use agent::{Agent, ToolRegistry};
use serde_json::{json, Value};
use tempfile::TempDir;

/// Sets up a completely isolated sandbox workspace initialized from `tests/fixtures/sample_project`.
fn setup_sandbox() -> (TempDir, PathBuf) {
    let temp = TempDir::new().expect("Failed to create temporary directory for sandbox");
    let sandbox_path = temp.path().join("sample_project");
    fs::create_dir_all(&sandbox_path).expect("Failed to create sandbox project dir");

    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample_project");

    copy_dir_recursive(&fixture_dir, &sandbox_path).expect("Failed to copy fixture to sandbox");
    (temp, sandbox_path)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Scripted mock provider that yields predefined sequences of events for each completion call.
struct ScriptedProvider {
    name: String,
    call_sequences: Mutex<Vec<Vec<ProviderEvent>>>,
}

impl ScriptedProvider {
    fn new(name: &str, sequences: Vec<Vec<ProviderEvent>>) -> Self {
        Self {
            name: name.to_string(),
            call_sequences: Mutex::new(sequences),
        }
    }
}

impl Provider for ScriptedProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn complete_stream(
        &self,
        _messages: &[Message],
        _tools: &[Value],
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<(), ProviderError> {
        let mut sequences = self.call_sequences.lock().unwrap();
        if sequences.is_empty() {
            on_event(ProviderEvent::TextDelta("No further response scripted.".into()));
            on_event(ProviderEvent::Done);
            return Ok(());
        }

        let events = sequences.remove(0);
        for ev in events {
            on_event(ev);
        }
        Ok(())
    }
}

#[test]
fn test_human_message_write_file_creates_and_verifies_on_disk() {
    let (_temp, sandbox) = setup_sandbox();
    let new_file_path = sandbox.join("src").join("utils.rs");
    assert!(!new_file_path.exists(), "Target file must not exist before write");

    let script = vec![
        // Turn 1.1: Model calls write_file
        vec![
            ProviderEvent::ToolCallComplete {
                id: "call_write_1".into(),
                name: "write_file".into(),
                args: json!({
                    "path": new_file_path.to_str().unwrap(),
                    "content": "pub fn greet(name: &str) -> String {\n    format!(\"Hello, {}!\", name)\n}\n"
                }),
                thought_signature: None,
            },
            ProviderEvent::Done,
        ],
        // Turn 1.2: After seeing tool result, model responds with explanation to human
        vec![
            ProviderEvent::TextDelta("I have created `src/utils.rs` with the requested `greet` function.".into()),
            ProviderEvent::Done,
        ],
    ];

    let provider = Box::new(ScriptedProvider::new("MockModel", script));
    let mut agent = Agent::with_workspace(provider, ToolRegistry::all(), &sandbox);

    let mut events = Vec::new();
    let mut on_event = |ev: AgentEvent| events.push(ev);
    let mut ask_approval = |_req: ApprovalRequest| ApprovalDecision::Approve;

    let human_message = "Please create a new utility module in src/utils.rs that provides a greet function.";
    let reply = agent.run(human_message, &mut on_event, &mut ask_approval).expect("Agent execution failed");

    // 1. Verify model response
    assert_eq!(reply, "I have created `src/utils.rs` with the requested `greet` function.");

    // 2. Verify file exists on real disk in sandbox
    assert!(new_file_path.exists(), "src/utils.rs was not created on disk");
    let content = fs::read_to_string(&new_file_path).expect("Failed to read created file");
    assert!(content.contains("pub fn greet(name: &str) -> String"));

    // 3. Verify event stream tracked tool execution
    let started = events.iter().any(|e| matches!(e, AgentEvent::ToolStarted { name, .. } if name == "write_file"));
    let finished = events.iter().any(|e| matches!(e, AgentEvent::ToolFinished { name, .. } if name == "write_file"));
    assert!(started && finished, "Expected write_file tool events to be recorded");
}

#[test]
fn test_human_message_edit_file_patches_existing_code() {
    let (_temp, sandbox) = setup_sandbox();
    let main_rs_path = sandbox.join("src").join("main.rs");
    let original_content = fs::read_to_string(&main_rs_path).expect("Failed to read main.rs");
    assert!(original_content.contains("sample_project::add(10, 20)"));

    let script = vec![
        // Turn 1.1: Model calls edit_file with surgical patch
        vec![
            ProviderEvent::ToolCallComplete {
                id: "call_edit_1".into(),
                name: "edit_file".into(),
                args: json!({
                    "path": main_rs_path.to_str().unwrap(),
                    "target": "sample_project::add(10, 20)",
                    "replacement": "sample_project::add(40, 2)"
                }),
                thought_signature: None,
            },
            ProviderEvent::Done,
        ],
        // Turn 1.2: Model answers human
        vec![
            ProviderEvent::TextDelta("I updated `main.rs` to compute add(40, 2).".into()),
            ProviderEvent::Done,
        ],
    ];

    let provider = Box::new(ScriptedProvider::new("MockModel", script));
    let mut agent = Agent::with_workspace(provider, ToolRegistry::all(), &sandbox);

    let mut events = Vec::new();
    let mut on_event = |ev: AgentEvent| events.push(ev);
    let mut ask_approval = |_req: ApprovalRequest| ApprovalDecision::Approve;

    let human_message = "Update src/main.rs to calculate add(40, 2) instead of 10 and 20.";
    let reply = agent.run(human_message, &mut on_event, &mut ask_approval).expect("Agent execution failed");

    assert_eq!(reply, "I updated `main.rs` to compute add(40, 2).");

    // Verify on disk modification
    let modified = fs::read_to_string(&main_rs_path).expect("Failed to read updated main.rs");
    assert!(modified.contains("sample_project::add(40, 2)"));
    assert!(!modified.contains("sample_project::add(10, 20)"));
}

#[test]
fn test_human_message_read_file_inspects_source() {
    let (_temp, sandbox) = setup_sandbox();
    let lib_rs_path = sandbox.join("src").join("lib.rs");

    let script = vec![
        // Turn 1.1: Model calls read_file
        vec![
            ProviderEvent::ToolCallComplete {
                id: "call_read_1".into(),
                name: "read_file".into(),
                args: json!({
                    "path": lib_rs_path.to_str().unwrap()
                }),
                thought_signature: None,
            },
            ProviderEvent::Done,
        ],
        // Turn 1.2: Model summarizes functions
        vec![
            ProviderEvent::TextDelta("`src/lib.rs` exports `add` and `subtract` arithmetic functions.".into()),
            ProviderEvent::Done,
        ],
    ];

    let provider = Box::new(ScriptedProvider::new("MockModel", script));
    let mut agent = Agent::with_workspace(provider, ToolRegistry::all(), &sandbox);

    let mut events = Vec::new();
    let mut on_event = |ev: AgentEvent| events.push(ev);
    let mut ask_approval = |_req: ApprovalRequest| ApprovalDecision::Approve;

    let human_message = "What functions are defined in src/lib.rs?";
    let reply = agent.run(human_message, &mut on_event, &mut ask_approval).expect("Agent execution failed");

    assert_eq!(reply, "`src/lib.rs` exports `add` and `subtract` arithmetic functions.");
    let read_event_found = events.iter().any(|e| matches!(e, AgentEvent::ToolStarted { name, .. } if name == "read_file"));
    assert!(read_event_found);
}

#[test]
fn test_human_message_multi_turn_lifecycle_write_edit_and_read() {
    let (_temp, sandbox) = setup_sandbox();
    let config_path = sandbox.join("config.json");

    let script = vec![
        // --- TURN 1 ---
        // 1.1: Call write_file
        vec![
            ProviderEvent::ToolCallComplete {
                id: "call_write_cfg".into(),
                name: "write_file".into(),
                args: json!({
                    "path": config_path.to_str().unwrap(),
                    "content": "{\n  \"port\": 8080,\n  \"name\": \"app\"\n}\n"
                }),
                thought_signature: None,
            },
            ProviderEvent::Done,
        ],
        // 1.2: Answer turn 1
        vec![
            ProviderEvent::TextDelta("Created `config.json` on port 8080.".into()),
            ProviderEvent::Done,
        ],

        // --- TURN 2 ---
        // 2.1: Call edit_file
        vec![
            ProviderEvent::ToolCallComplete {
                id: "call_edit_cfg".into(),
                name: "edit_file".into(),
                args: json!({
                    "path": config_path.to_str().unwrap(),
                    "target": "\"port\": 8080",
                    "replacement": "\"port\": 9000"
                }),
                thought_signature: None,
            },
            ProviderEvent::Done,
        ],
        // 2.2: Answer turn 2
        vec![
            ProviderEvent::TextDelta("Updated port to 9000 in `config.json`.".into()),
            ProviderEvent::Done,
        ],

        // --- TURN 3 ---
        // 3.1: Call read_file
        vec![
            ProviderEvent::ToolCallComplete {
                id: "call_read_cfg".into(),
                name: "read_file".into(),
                args: json!({
                    "path": config_path.to_str().unwrap()
                }),
                thought_signature: None,
            },
            ProviderEvent::Done,
        ],
        // 3.2: Answer turn 3
        vec![
            ProviderEvent::TextDelta("Confirmed: `config.json` contains port 9000.".into()),
            ProviderEvent::Done,
        ],
    ];

    let provider = Box::new(ScriptedProvider::new("MockModel", script));
    let mut agent = Agent::with_workspace(provider, ToolRegistry::all(), &sandbox);

    let mut events = Vec::new();
    let mut on_event = |ev: AgentEvent| events.push(ev);
    let mut ask_approval = |_req: ApprovalRequest| ApprovalDecision::Approve;

    // Turn 1: Human asks to write
    let r1 = agent.run("Please create config.json with port 8080.", &mut on_event, &mut ask_approval).unwrap();
    assert_eq!(r1, "Created `config.json` on port 8080.");
    assert!(config_path.exists());

    // Turn 2: Human asks to edit
    let r2 = agent.run("Please change the port to 9000.", &mut on_event, &mut ask_approval).unwrap();
    assert_eq!(r2, "Updated port to 9000 in `config.json`.");
    let cfg_content = fs::read_to_string(&config_path).unwrap();
    assert!(cfg_content.contains("\"port\": 9000"));

    // Turn 3: Human asks to read & verify
    let r3 = agent.run("Please read back config.json to confirm.", &mut on_event, &mut ask_approval).unwrap();
    assert_eq!(r3, "Confirmed: `config.json` contains port 9000.");

    // Verify conversation history retained all 3 turns
    assert!(agent.conversation().len() >= 6);
}

#[test]
fn test_human_message_pure_conversation_calls_zero_tools() {
    let (_temp, sandbox) = setup_sandbox();

    let script = vec![
        vec![
            ProviderEvent::TextDelta("In Rust, an immutable borrow (&T) allows shared read-only access, while a mutable borrow (&mut T) guarantees exclusive write access at compile time.".into()),
            ProviderEvent::Done,
        ],
    ];

    let provider = Box::new(ScriptedProvider::new("MockModel", script));
    let mut agent = Agent::with_workspace(provider, ToolRegistry::all(), &sandbox);

    let mut events = Vec::new();
    let mut on_event = |ev: AgentEvent| events.push(ev);
    let mut ask_approval = |_req: ApprovalRequest| ApprovalDecision::Approve;

    let human_message = "What is the difference between an immutable borrow and a mutable borrow in Rust?";
    let reply = agent.run(human_message, &mut on_event, &mut ask_approval).expect("Agent execution failed");

    assert!(reply.contains("exclusive write access"));
    // Verify that NO tool events were fired: the LLM decided purely conversationally!
    let tools_called = events.iter().any(|e| matches!(e, AgentEvent::ToolStarted { .. }));
    assert!(!tools_called, "Pure conversational prompt should not invoke any tools");
}

#[test]
fn test_human_benchmark_json_specification() {
    let json_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("human_tests.json");
    assert!(json_path.exists(), "human_tests.json must exist in tests/");

    let data = fs::read_to_string(&json_path).expect("Failed to read human_tests.json");
    let parsed: serde_json::Value = serde_json::from_str(&data).expect("human_tests.json must be valid JSON");

    assert_eq!(parsed["total"].as_u64(), Some(30));
    let tests = parsed["tests"].as_array().expect("tests array expected");
    assert_eq!(tests.len(), 30);

    for (i, t) in tests.iter().enumerate() {
        assert_eq!(t["id"].as_u64(), Some((i + 1) as u64));
        assert!(t["human_message"].as_str().is_some());
        assert!(t["expected_tools"].as_array().is_some());
        assert!(t["expected_behavior"].as_str().is_some());
    }
}

#[test]
#[ignore = "live provider test: burns quota"]
fn test_live_groq_snake_game_human_message() {
    let snake_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snake_game");

    if let Some(mut agent) = agent::create_agent_for_model_and_workspace("groq", "openai/gpt-oss-120b", &snake_dir) {
        let mut on_event = |ev: AgentEvent| {
            println!("[Agent Event] {:?}", ev);
        };
        let mut ask_approval = |_req: ApprovalRequest| ApprovalDecision::Approve;

        let human_prompt = "Where is the snake game loop defined in src/App.jsx and how does it detect collisions?";
        let res = agent.run(human_prompt, &mut on_event, &mut ask_approval);
        println!("Live Groq Agent Result: {:?}", res);
        if let Ok(reply) = res {
            println!("Agent Reply: {}", reply);
            assert!(!reply.is_empty(), "Reply must not be empty");
        }
    }
}

#[test]
#[ignore = "live provider test: requires GEMINI_API_KEY"]
fn test_live_gemini_human_message_loop() {
    let (_temp, sandbox) = setup_sandbox();
    if let Some(mut agent) = agent::create_agent_for_model_and_workspace("gemini", "gemini-3.5-flash", &sandbox) {
        let mut events = Vec::new();
        let mut on_event = |ev: AgentEvent| {
            println!("[Agent Event] {:?}", ev);
            events.push(ev);
        };
        let mut ask_approval = |_req: ApprovalRequest| ApprovalDecision::Approve;

        let human_message = "Read Cargo.toml in this project and tell me the package name and version.";
        let res = agent.run(human_message, &mut on_event, &mut ask_approval);
        println!("Live Gemini Agent Result: {:?}", res);
        assert!(res.is_ok(), "Live agent failed: {:?}", res.err());
        let reply = res.unwrap();
        println!("Live agent reply: {}", reply);
        assert!(!reply.is_empty());
    }
}


