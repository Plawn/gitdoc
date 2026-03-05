/// Rough token estimate: ~4 chars per token.
pub fn estimate_str_tokens(s: &str) -> usize {
    s.len() / 4
}

pub fn estimate_tokens(question: &str, answer: &str) -> i32 {
    estimate_str_tokens(question) as i32 + estimate_str_tokens(answer) as i32
}

pub struct PromptBudgetBreakdown {
    pub max_tokens: usize,
    pub available: usize,
    pub question_tokens: usize,
    pub cheatsheet_tokens: usize,
    pub architect_tokens: usize,
    pub code_context_tokens: usize,
    pub recent_turns_tokens: usize,
    pub condensed_tokens: usize,
    pub total_used: usize,
}

/// Build the user message with a token budget.
///
/// Allocation priority (highest first):
/// 1. Question — always full
/// 2. Cheatsheet — always full
/// 2.5. Architect context — always full (short)
/// 3. Code context — truncate entries from the end if over budget
/// 4. Recent turns — drop oldest first if over budget
/// 5. Condensed context — hard-truncate to remaining budget
pub fn build_conversation_user_message_with_budget(
    cheatsheet: &str,
    architect_context: &str,
    condensed_context: &str,
    recent_turns: &[crate::db::ConversationTurnRow],
    code_context: &str,
    question: &str,
    max_tokens: usize,
) -> (String, PromptBudgetBreakdown) {
    const SYSTEM_RESERVE: usize = 200;
    const ANSWER_RESERVE: usize = 3000;

    let available = max_tokens.saturating_sub(SYSTEM_RESERVE + ANSWER_RESERVE);

    // Priority 1 & 2: question and cheatsheet are always included
    let question_section = format!("## Current question\n{}", question);
    let cheatsheet_section = if !cheatsheet.is_empty() {
        format!("## Repo cheatsheet\n{}\n\n", cheatsheet)
    } else {
        String::new()
    };

    // Priority 2.5: architect context — always included in full (short)
    let architect_section = if !architect_context.is_empty() {
        format!("{}\n", architect_context)
    } else {
        String::new()
    };

    let fixed_tokens = estimate_str_tokens(&question_section)
        + estimate_str_tokens(&cheatsheet_section)
        + estimate_str_tokens(&architect_section);
    let mut remaining = available.saturating_sub(fixed_tokens);

    // Priority 3: code context — split by entries (### boundaries), drop from end
    let code_section = if !code_context.is_empty() {
        let header = "## Relevant code context\n";
        let header_tokens = estimate_str_tokens(header);
        if remaining > header_tokens {
            let budget_for_code = remaining - header_tokens;
            let entries: Vec<&str> = code_context.split("\n### ").collect();
            let mut kept = String::new();
            let mut used = 0;
            for (i, entry) in entries.iter().enumerate() {
                let full_entry = if i == 0 { entry.to_string() } else { format!("### {}", entry) };
                let entry_tokens = estimate_str_tokens(&full_entry);
                if used + entry_tokens > budget_for_code {
                    break;
                }
                kept.push_str(&full_entry);
                if !full_entry.ends_with('\n') {
                    kept.push('\n');
                }
                used += entry_tokens;
            }
            if !kept.is_empty() {
                remaining = remaining.saturating_sub(header_tokens + used);
                format!("{}{}\n", header, kept.trim_end())
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Priority 4: recent turns — drop oldest first
    let turns_section = if !recent_turns.is_empty() {
        let header = "## Recent conversation\n";
        let header_tokens = estimate_str_tokens(header);
        if remaining > header_tokens {
            let budget_for_turns = remaining - header_tokens;
            // Build turn strings from newest to oldest, then reverse the kept ones
            let mut turn_strings: Vec<String> = Vec::new();
            let mut used = 0;
            for turn in recent_turns.iter().rev() {
                let s = format!("**Q:** {}\n**A:** {}\n\n", turn.question, turn.answer);
                let t = estimate_str_tokens(&s);
                if used + t > budget_for_turns {
                    break;
                }
                turn_strings.push(s);
                used += t;
            }
            turn_strings.reverse();
            if !turn_strings.is_empty() {
                remaining = remaining.saturating_sub(header_tokens + used);
                let mut section = header.to_string();
                for s in &turn_strings {
                    section.push_str(s);
                }
                section
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Priority 5: condensed context — hard-truncate to remaining budget
    let condensed_section = if !condensed_context.is_empty() && remaining > 0 {
        let header = "## Previous conversation summary\n";
        let header_tokens = estimate_str_tokens(header);
        if remaining > header_tokens {
            let budget_for_condensed = remaining - header_tokens;
            let max_chars = budget_for_condensed * 4; // reverse of token estimate
            let truncated = if condensed_context.len() > max_chars {
                &condensed_context[..max_chars]
            } else {
                condensed_context
            };
            format!("{}{}\n\n", header, truncated)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Assemble in reading order
    let mut user_content = String::new();
    user_content.push_str(&cheatsheet_section);
    user_content.push_str(&architect_section);
    user_content.push_str(&condensed_section);
    user_content.push_str(&turns_section);
    user_content.push_str(&code_section);
    user_content.push_str(&question_section);

    let question_tokens = estimate_str_tokens(&question_section);
    let cheatsheet_tokens = estimate_str_tokens(&cheatsheet_section);
    let architect_tokens = estimate_str_tokens(&architect_section);
    let code_context_tokens = estimate_str_tokens(&code_section);
    let recent_turns_tokens = estimate_str_tokens(&turns_section);
    let condensed_tokens = estimate_str_tokens(&condensed_section);
    let total_used = question_tokens + cheatsheet_tokens + architect_tokens + code_context_tokens + recent_turns_tokens + condensed_tokens;

    let breakdown = PromptBudgetBreakdown {
        max_tokens,
        available,
        question_tokens,
        cheatsheet_tokens,
        architect_tokens,
        code_context_tokens,
        recent_turns_tokens,
        condensed_tokens,
        total_used,
    };

    (user_content, breakdown)
}
