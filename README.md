# zene

Autonomous multi-turn coding agent in Rust. Tree-sitter AST perception, 13 tools, 3-tier skill system, and pluggable LLM providers (Gemini, Groq).

Built as the intelligence layer inside [Zenthree](https://zenthralabs.com/products/zenthra/apps/zenthree) — open-sourced for developers building autonomous coding agents or terminal workflows.

---

## Installation & CLI Quickstart

### 1. Build and install globally

```bash
git clone git@github.com:kabirajpan/zene.git
cd zene

# Installs the `zene` and `zene-agent` binaries into ~/.cargo/bin
cargo install --path .
```

Make sure `~/.cargo/bin` is in your `PATH`.

### 2. Configure your API key

ZENE supports **Google Gemini** (recommended) and **Groq**:

```bash
# Option A: In your current shell or ~/.bashrc / ~/.zshrc
export GEMINI_API_KEY="your-gemini-key"
# or
export GROQ_API_KEY="your-groq-key"

# Option B: Global user config
mkdir -p ~/.config/zene
echo 'GEMINI_API_KEY="your-gemini-key"' > ~/.config/zene/.env

# Option C: Local project .env
echo 'GEMINI_API_KEY="your-gemini-key"' > .env
```

Free API keys:
- **Gemini**: [https://aistudio.google.com/app/apikey](https://aistudio.google.com/app/apikey)
- **Groq**: [https://console.groq.com/keys](https://console.groq.com/keys)

### 3. Run ZENE

```bash
# Interactive REPL mode in current directory
zene

# One-shot command on current directory
zene "Explore this repository and explain the project structure"

# Target a specific workspace
zene -w /path/to/project "Find and fix compiler warnings in src/main.rs"

# Verbose mode (shows full tool inputs, outputs, and sub-millisecond reflex gate)
zene -v "Implement a health check endpoint in main.rs"

# Switch model or provider
zene -p groq -m openai/gpt-oss-120b "Write unit tests for the parser"
zene -p gemini -m gemini-3.5-flash-lite
```

Inside the interactive REPL:
- `:help` — Show commands
- `:verbose` — Toggle telemetry / raw tool trace
- `:model <name>` — Switch model live
- `:workspace <path>` — Change target directory
- `:clear` — Reset conversation context
- `:quit` or `:exit` — Exit session

---

## What it does

`zene` runs an autonomous multi-turn agentic loop:

1. **Perception**: Scans files, AST tokens, diagnostics, and git state.
2. **Sensory Reflex Pre-Filter**: Connectome evaluates intent and safety locks in < 15 microseconds.
3. **Reasoning & Tool Selection**: Dispatches tasks to high-capacity reasoning models with structured function definitions tailored to the turn.
4. **Execution & Approval Gate**: Executes filesystem mutations, searches, and terminal commands. Destructive actions can require interactive approval.
5. **Self-Correction Loop**: Catches compilation errors, test failures, or unknown tools and feeds them back into the context window until fixed.

---

## Bio-Connectome Sensory Reflex Engine (The "Fly Brain")

`zene` embeds a **500-neuron biological connectome neural network** modeled after the adult *Drosophila melanogaster* (fruit fly) neural connectome (3,889 synapses, stored as an embedded 154 KB binary).

### Why is it in the agent?

Conventional LLM coding agents send full tool schemas to cloud APIs on every turn and rely entirely on the cloud model to decide whether to mutate files or execute commands. This creates three critical problems:
1. **Safety vulnerabilities**: Models can hallucinate destructive shell commands (`rm -rf /`, `git reset --hard`, destructive drive formatting).
2. **Context & Token Bloat**: Sending 13+ tool definitions on simple conceptual queries (*"how does this algorithm work?"*) wastes context window and increases API cost.
3. **Tool Hallucinations**: Models often call filesystem search or file creation tools when the user simply said *"hello"*.

### What is it helping?

The Fly Brain acts as an **involuntary biological reflex gate** that runs locally *before* the remote LLM:

1. **⚡ Sub-15 Microsecond Execution (< 0.015 ms)**:
   Executes completely natively in pure Rust using a 30-step Euler numerical integration over the connectome RNN matrix. It requires **zero external runtimes** (no ONNX, no PyTorch, no LibTorch), zero background daemons, and zero network calls.

2. **🚨 Giant Fiber Safety Reflex Lock**:
   Modeled after the fruit fly's *giant fiber escape reflex*—an involuntary circuit that fires in milliseconds when a shadow descends—the connectome intercepts catastrophic workspace wipe commands in **< 15 µs**. Workspace buffer mutation is locked *before the prompt is ever transmitted to the LLM*.

3. **🎯 Dynamic Intent & Tool Provisioning**:
   Classifies incoming user intent into 5 discrete neural firing states to inject only the relevant tools into the conversation turn:
   - **`DISCUSSION`**: Conceptual questions, architecture, greetings → Locks mutating tools; model cannot touch workspace files.
   - **`INSPECTION`**: *"Where is X"*, *"find usages"*, *"git diff"* → Provisions high-speed read-only search tools (`read_file`, `list_directory`, `search`, `git_diff`).
   - **`PLANNING`**: Architecture and refactoring roadmaps → Provisions planning graph tools.
   - **`EXECUTION`**: *"Fix the compiler error"*, *"implement feature"*, *"run tests"* → Provisions full code mutation, edit, and terminal toolsets.
   - **`REFLEX_LOCK`**: Catastrophic destruction → Disengages all tools and blocks execution.

## Architecture

```
src/
├── agentic_loop/   # Core engine — turn-by-turn multi-turn orchestration loop
├── ast/            # Tree-sitter AST engine for syntax-aware context
├── context/        # Context window manager, git diffs, & conversation pruner
├── orchestrator/   # BioReflexClient, intent classification, and session state
├── provider/       # Pluggable LLM providers (Gemini, Groq)
├── skills/         # 3-tier skill discovery & dynamic activation
├── tools/          # Tool registry + all 13 tool implementations
├── traits/         # Provider and Tool abstractions
├── types/          # Message, ToolCall, AgentEvent, ApprovalRequest types
└── verification/   # Post-tool verification & diagnostics runner
```

---

## Tools

All 13 tools are implemented natively in Rust:

| Tool | Category | Action Type | Description |
|---|---|---|---|
| `read_file` | Filesystem | Read-only | Reads file contents with line slicing |
| `write_file` | Filesystem | Destructive | Writes or creates files |
| `edit_file` | Filesystem | Destructive | Targeted substring search & replace edits |
| `delete_file` | Filesystem | Destructive | Deletes files with safety checks |
| `rename_file` | Filesystem | Destructive | Renames or moves files |
| `list_directory` | Filesystem | Read-only | Recursive directory tree listing |
| `search` | Search | Read-only | High-speed ripgrep-style content & filename search |
| `run_terminal` | Terminal | Destructive | Executes shell commands in the workspace |
| `get_diagnostics` | Diagnostics | Read-only | Captures live compiler / linter diagnostics |
| `git_status` | Git | Read-only | Git repository status |
| `git_diff` | Git | Read-only | Git unstaged and staged diffs |
| `create_plan` | Planning | Read-only | Initializes multi-step task plans |
| `update_plan_step` | Planning | Read-only | Updates execution status of plan steps |

---

## Skills System

Skills are `SKILL.md` documents with YAML frontmatter that inject domain-specific context into the agent's instructions dynamically:

```
1. Global  — Built-in skills compiled into the binary (debug, explore, refactor, test, docs, git)
2. User    — ~/.zene/skills/<name>/SKILL.md (or ~/.zenthree/skills/)
3. Project — <workspace>/.zene/skills/<name>/SKILL.md (or .zenthree/skills/)
```

Project skills take precedence over User skills, which take precedence over Global skills.

---

## Using as a Rust Library

You can embed `zene` as a library inside your own Rust tools:

```toml
[dependencies]
agent = { git = "https://github.com/kabirajpan/zene.git" }
```

```rust
use agent::{create_agent_for_model_and_workspace, types::ApprovalDecision};

fn main() {
    let mut agent = create_agent_for_model_and_workspace(
        "gemini",
        "gemini-3.5-flash-lite",
        "./",
    ).expect("API credentials not found");

    let mut on_event = |ev| println!("{ev:?}");
    let mut ask_approval = |_req| ApprovalDecision::Approve;

    let response = agent.run("Find and fix warnings in src/main.rs", &mut on_event, &mut ask_approval);
    println!("{}", response.unwrap());
}
```

---

## Testing API Connectivity

Test that your keys and network endpoints are operational:

```bash
./test-models.sh
```

---

## License

[Apache 2.0](LICENSE) — free for personal and commercial open-source use.
Part of [ZenthraLabs](https://zenthralabs.com) open research.
