use crate::types::message::Message;

/// Manages context window pruning, safety capping, and history bounding.
pub struct ContextManager;

impl ContextManager {
    /// Prune older intermediate messages if history exceeds the configured cap,
    /// while ALWAYS preserving the system prompt, the most recent user request(s),
    /// and keeping tool calls and tool responses valid.
    pub fn prune_history(conversation: &mut Vec<Message>, max_messages: usize) {
        if conversation.len() <= max_messages {
            return;
        }

        let overflow = conversation.len() - max_messages;
        let has_system = conversation
            .first()
            .map(|m| m.role == crate::types::message::Role::System)
            .unwrap_or(false);

        // Preserve system instruction and root task
        let preserve_head = if has_system {
            if conversation.len() > 1 && conversation[1].role == crate::types::message::Role::User {
                2
            } else {
                1
            }
        } else {
            1
        };

        // Find the active task: the latest user message
        let last_user_idx = conversation
            .iter()
            .rposition(|m| m.role == crate::types::message::Role::User)
            .unwrap_or(0);

        // Protected tail start: NEVER delete the active user message or the most recent turns
        let min_recent_keep = (max_messages / 2).max(4);
        let tail_by_recency = conversation.len().saturating_sub(min_recent_keep);
        let preserve_tail_start = if last_user_idx > preserve_head {
            last_user_idx
        } else {
            tail_by_recency.max(preserve_head)
        };

        if preserve_tail_start > preserve_head {
            let max_drainable = preserve_tail_start - preserve_head;
            let remove_count = overflow.min(max_drainable);

            let mut drain_end = preserve_head + remove_count;
            while drain_end < conversation.len() && conversation[drain_end].role == crate::types::message::Role::Tool {
                drain_end += 1;
            }

            if drain_end > preserve_head && drain_end <= conversation.len() {
                conversation.drain(preserve_head..drain_end);
            }
        } else if conversation.len() > preserve_head + 2 {
            let max_drainable = conversation.len().saturating_sub(preserve_head + 2);
            let remove_count = overflow.min(max_drainable);
            let mut drain_end = preserve_head + remove_count;
            while drain_end < conversation.len() && conversation[drain_end].role == crate::types::message::Role::Tool {
                drain_end += 1;
            }
            if drain_end > preserve_head && drain_end < conversation.len() {
                conversation.drain(preserve_head..drain_end);
            }
        }
    }

    /// Enforce a hard cap on tool output to prevent massive dumps from blowing out LLM context limits.
    /// Preserves the head and tail of the output with an informative truncation marker, safely respecting UTF-8 boundaries.
    pub fn sanitize_tool_result(output: &str, max_chars: usize) -> String {
        if output.len() <= max_chars {
            return output.to_string();
        }

        let head_target = max_chars / 2;
        let head_end = output
            .char_indices()
            .map(|(idx, _)| idx)
            .take_while(|&idx| idx <= head_target)
            .last()
            .unwrap_or(0);

        let tail_target = output.len().saturating_sub(max_chars / 4);
        let tail_start = output
            .char_indices()
            .map(|(idx, _)| idx)
            .find(|&idx| idx >= tail_target)
            .unwrap_or(output.len());

        let head = &output[..head_end];
        let tail = &output[tail_start..];

        format!(
            "{}\n\n... [Output truncated: {} total characters. Showing head and tail to prevent context overflow] ...\n\n{}",
            head.trim_end(),
            output.len(),
            tail.trim_start()
        )
    }

    /// Compact historical tool outputs in older turns so they do not consume context window tokens
    /// on subsequent turns, while preserving the active turn's tool outputs intact so the model
    /// retains working memory of files inspected during this task.
    pub fn compact_prior_tool_results(conversation: &mut [Message]) {
        if conversation.len() <= 2 {
            return;
        }

        // Identify the start of the active user turn
        let last_user_idx = conversation
            .iter()
            .rposition(|m| m.role == crate::types::message::Role::User)
            .unwrap_or(0);

        let total_chars: usize = conversation.iter().map(|m| m.content.len()).sum();

        // Under normal circumstances (< 20k chars), ONLY compact tools from prior turns before last_user_idx.
        // If context approaches low-tier token limits (> 20k chars ~ 5,000 tokens), allow compacting older tools within the turn,
        // but always leave the last 4 messages completely intact.
        let compact_cutoff = if total_chars > 20_000 {
            conversation.len().saturating_sub(4)
        } else {
            last_user_idx
        };

        for msg in &mut conversation[..compact_cutoff] {
            if msg.role == crate::types::message::Role::Tool && msg.content.len() > 400 {
                // If this was a file inspection output, preserve the file header and line count
                let summary = if msg.content.starts_with("[file:") {
                    let first_line = msg.content.lines().next().unwrap_or("[file read]");
                    format!("{} (full content held in memory from prior step)]", first_line.trim_end_matches(']'))
                } else {
                    let preview: String = msg.content.chars().take(100).collect();
                    format!(
                        "[Prior tool output ({} chars) summarized: {}...]",
                        msg.content.len(),
                        preview.trim()
                    )
                };
                msg.content = summary;
            }
        }
    }

    /// Emergency compaction when a 413 Payload Too Large or token rate limit is encountered.
    /// Retains the system prompt and the active user prompt + turn, only removing older intermediate history.
    pub fn emergency_prune(conversation: &mut Vec<Message>) {
        let has_system = conversation
            .first()
            .map(|m| m.role == crate::types::message::Role::System)
            .unwrap_or(false);
        let start_idx = if has_system { 1 } else { 0 };

        if let Some(last_user_idx) = conversation
            .iter()
            .rposition(|m| m.role == crate::types::message::Role::User)
        {
            if last_user_idx > start_idx {
                conversation.drain(start_idx..last_user_idx);
            }
        }

        // On emergency (413 Payload Too Large), aggressively compact earlier tools in the active turn
        // keeping only the 2 most recent messages full to recover under provider TPM limits immediately.
        let cutoff = conversation.len().saturating_sub(2);
        for msg in &mut conversation[..cutoff] {
            if msg.role == crate::types::message::Role::Tool && msg.content.len() > 200 {
                let summary = if msg.content.starts_with("[file:") {
                    let first_line = msg.content.lines().next().unwrap_or("[file read]");
                    format!("{} (full content held in memory from prior step)]", first_line.trim_end_matches(']'))
                } else {
                    let preview: String = msg.content.chars().take(80).collect();
                    format!("[Output summarized: {}...]", preview.trim())
                };
                msg.content = summary;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::message::Role;

    #[test]
    fn test_prune_history_preserves_latest_user_message() {
        let mut conv = vec![
            Message::system("sys"),
            Message::user("turn 1 query"),
            Message::assistant("a1"),
            Message::tool_result("t1", "res1"),
            Message::user("turn 2 - dark theme"),
            Message::assistant("a2"),
            Message::tool_result("t2", "res2"),
        ];

        ContextManager::prune_history(&mut conv, 5);

        assert_eq!(conv[0].role, Role::System);
        // The active task (turn 2) MUST be preserved!
        let has_active_user = conv.iter().any(|m| m.role == Role::User && m.content == "turn 2 - dark theme");
        assert!(has_active_user, "Active user request must never be pruned");
    }

    #[test]
    fn test_emergency_prune_preserves_system_and_active_user() {
        let mut conv = vec![
            Message::system("system instructions"),
            Message::user("old user prompt"),
            Message::assistant("old assistant"),
            Message::tool_result("call_1", "old tool output"),
            Message::user("latest active user request"),
            Message::assistant("current assistant"),
            Message::tool_result("call_2", "current tool output"),
        ];

        ContextManager::emergency_prune(&mut conv);

        assert_eq!(conv[0].role, Role::System);
        assert_eq!(conv[1].role, Role::User);
        assert_eq!(conv[1].content, "latest active user request");
        assert_eq!(conv[2].role, Role::Assistant);
        assert_eq!(conv[3].role, Role::Tool);
    }

    #[test]
    fn test_compact_prior_tool_results_preserves_active_turn_memory() {
        let big_code = "[file: src/main.rs (120 lines)]\n".to_string() + &"let x = 1;\n".repeat(50);
        let mut conv = vec![
            Message::system("sys"),
            Message::user("first question"),
            Message::assistant("a1"),
            Message::tool_result("call_old", &big_code),
            Message::user("second question: explain project"),
            Message::assistant("a2"),
            Message::tool_result("call_active_1", &big_code),
            Message::assistant("a3"),
            Message::tool_result("call_active_2", &big_code),
        ];

        ContextManager::compact_prior_tool_results(&mut conv);

        // Older turn tool result before second question should be summarized:
        assert!(conv[3].content.contains("full content held in memory from prior step"));
        // Active turn tool results MUST REMAIN INTACT!
        assert!(conv[6].content.contains("let x = 1;"));
        assert!(conv[8].content.contains("let x = 1;"));
    }
}
