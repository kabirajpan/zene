# zene

Autonomous multi-turn coding agent in Rust. Tree-sitter AST perception, 13 tools, 3-tier skill system, and pluggable LLM providers (Gemini, Groq).

Built as the intelligence layer inside [Zenthree](https://zenthralabs.com/products/zenthra/apps/zenthree) — extracted here as a standalone crate.

---

## What it does

`zene` runs an agentic loop that:

1. Receives a prompt
2. Calls an LLM provider with a tool schema
3. Executes whichever tools the model requests (filesystem, search, terminal, git, planning)
4. Feeds results back into the conversation
5. Repeats until the model produces a final text response — or a loop/context cap is hit

It handles the full execution spec: multi-tool batching, destructive action approval gates, error feedback as data, unknown tool recovery, network retry with backoff, context window pruning, and session export/import for persistence across restarts.

---

## Architecture

```
src/
├── agentic_loop/   # Core engine — the turn-by-turn orchestration loop
├── ast/            # Tree-sitter AST engine for code-aware context
├── context/        # Context window manager & conversation pruner
├── orchestrator/   # AgentConfig, session persistence, BioReflexClient stub
├── provider/       # LLM provider trait + Gemini & Groq implementations
├── skills/         # 3-tier skill discovery & activation (Global → User → Project)
├── tools/          # Tool registry + all 13 tool implementations
├── traits/         # Provider and Tool traits
├── types/          # Shared types: Message, ToolCall, AgentEvent, ApprovalRequest
└── verification/   # Post-tool verification runner
```

---

## Tools

| Tool | Category | Risk |
|---|---|---|
| `read_file` | Filesystem | Read-only |
| `write_file` | Filesystem | Destructive |
| `edit_file` | Filesystem | Destructive |
| `list_directory` | Filesystem | Read-only |
| `delete_file` | Filesystem | Destructive |
| `rename_file` | Filesystem | Destructive |
| `search` | Search | Read-only |
| `run_terminal` | Terminal | Destructive |
| `get_diagnostics` | Diagnostics | Read-only |
| `git_status` | Git | Read-only |
| `git_diff` | Git | Read-only |
| `create_plan` | Planning | Read-only |
| `update_plan_step` | Planning | Read-only |

Destructive tools trigger an approval gate — the caller decides whether to approve, reject, or modify. The engine feeds rejections back as data so the model can recover.

---

## Skills System

Skills are markdown files (`SKILL.md`) with YAML frontmatter that inject domain context into the agent's system prompt on demand. Discovered across 3 tiers (highest wins):

```
1. Global  — built-in skills compiled into the binary (debug, explore, + 4 more)
2. User    — ~/.zenthree/skills/<skill-name>/SKILL.md
3. Project — <workspace>/.zenthree/skills/<skill-name>/SKILL.md
```

The model activates skills by calling the `activate_skill` tool. Project skills override user skills, which override global ones.

---

## Providers

| Provider | Env var | Default model |
|---|---|---|
| Groq | `GROQ_API_KEY` | `llama-3.3-70b-versatile` |
| Gemini | `GEMINI_API_KEY` | `gemini-2.0-flash` |

Key resolution order: environment variable → `.env` file in the current directory.

Provider selection: set one or both keys. Groq is tried first (lower latency), falls back to Gemini automatically.

---

## Quick start

```toml
# Cargo.toml
[dependencies]
agent = { path = "." }
```

```rust
use agent::{create_agent_for_model_and_workspace, types::ApprovalDecision};

fn main() {
    let mut agent = create_agent_for_model_and_workspace(
        "gemini",
        "gemini-2.0-flash",
        "/path/to/your/project",
    ).expect("API key not found");

    let mut on_event = |ev| println!("{ev:?}");
    let mut ask_approval = |_req| ApprovalDecision::Approve; // or show a UI prompt

    let reply = agent.run("Refactor the auth module to use a trait", &mut on_event, &mut ask_approval);
    println!("{}", reply.unwrap());
}
```

Set your key:

```bash
export GEMINI_API_KEY=your_key_here
# or
export GROQ_API_KEY=your_key_here
# or create a .env file in the working directory
```

---

## Agent modes

```rust
// Full toolset, auto-discovers workspace skills
create_agent_for_model_and_workspace("groq", "llama-3.3-70b-versatile", "./my-project");

// Full toolset, uses cwd as workspace
create_agent("gemini", "gemini-2.0-flash");

// Read-only tools only (read_file + get_diagnostics)
create_minimal_agent();

// No workspace, no filesystem tools — general chat
create_agent_without_workspace("gemini", "gemini-2.0-flash");
```

---

## Running the CLI

```bash
# Run with default provider (auto-detected from env)
cargo run --bin zene-agent

# Verbose mode
./run-verbose.sh

# Test providers
./test-models.sh
```

---

## License

Apache 2.0 — see [LICENSE](LICENSE).

Part of the [ZenthraLabs](https://zenthralabs.com) open research.
