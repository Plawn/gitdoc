pub struct DocInfo {
    pub title: Option<String>,
    pub chunks: Vec<DocChunk>,
}

#[derive(Debug, Clone)]
pub struct DocChunk {
    pub section_title: Option<String>,
    pub text: String,
}

const CHUNK_SIZE_LIMIT: usize = 2000;

/// Extract the title from a markdown file (first `# heading`) and split into chunks.
pub fn parse_doc(content: &str) -> DocInfo {
    let title = content.lines().find_map(|line| {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("# ") {
            Some(rest.trim().to_string())
        } else {
            None
        }
    });
    let chunks = chunk_doc(content);
    DocInfo { title, chunks }
}

/// Split markdown into chunks at `##` headings, flushing at ~2000 chars.
pub fn chunk_doc(content: &str) -> Vec<DocChunk> {
    if content.trim().is_empty() {
        return vec![];
    }

    let mut chunks = Vec::new();
    let mut current_title: Option<String> = None;
    let mut current_text = String::new();

    for line in content.lines() {
        let trimmed = line.trim();
        // Split at ## headings (but not # which is the doc title)
        if trimmed.starts_with("## ") {
            // Flush current chunk if non-empty
            if !current_text.trim().is_empty() {
                chunks.push(DocChunk {
                    section_title: current_title.take(),
                    text: current_text.trim().to_string(),
                });
            }
            current_title = Some(trimmed.strip_prefix("## ").unwrap().trim().to_string());
            current_text = String::new();
            current_text.push_str(line);
            current_text.push('\n');
            continue;
        }

        // Check if adding this line would exceed the limit
        if current_text.len() + line.len() + 1 > CHUNK_SIZE_LIMIT && !current_text.trim().is_empty() {
            chunks.push(DocChunk {
                section_title: current_title.take(),
                text: current_text.trim().to_string(),
            });
            current_text = String::new();
        }

        current_text.push_str(line);
        current_text.push('\n');
    }

    // Flush remaining
    if !current_text.trim().is_empty() {
        chunks.push(DocChunk {
            section_title: current_title,
            text: current_text.trim().to_string(),
        });
    }

    chunks
}

/// A code example extracted from a doc comment.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CodeExample {
    /// The language tag (e.g. "rust", "no_run", "ignore"), if any.
    pub language: Option<String>,
    /// The code content.
    pub code: String,
}

/// Extract fenced code blocks (``` ... ```) from a doc comment string.
/// Returns all code examples found, preserving order.
pub fn extract_code_examples(doc_comment: &str) -> Vec<CodeExample> {
    let mut examples = Vec::new();
    let mut in_block = false;
    let mut current_lang: Option<String> = None;
    let mut current_code = String::new();

    for line in doc_comment.lines() {
        let trimmed = line.trim().trim_start_matches("/// ").trim_start_matches("///");

        if !in_block && trimmed.starts_with("```") {
            in_block = true;
            let lang = trimmed.strip_prefix("```").unwrap().trim();
            current_lang = if lang.is_empty() {
                None
            } else {
                // Handle "rust,no_run" etc.
                Some(lang.split(',').next().unwrap_or(lang).to_string())
            };
            current_code.clear();
            continue;
        }

        if in_block && trimmed.starts_with("```") {
            in_block = false;
            examples.push(CodeExample {
                language: current_lang.take(),
                code: current_code.trim_end().to_string(),
            });
            current_code.clear();
            continue;
        }

        if in_block {
            current_code.push_str(trimmed);
            current_code.push('\n');
        }
    }

    examples
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_title() {
        let doc = "# Hello World\n\nSome content.";
        let info = parse_doc(doc);
        assert_eq!(info.title.as_deref(), Some("Hello World"));
    }

    #[test]
    fn no_title() {
        let doc = "Just some text\nwithout a heading.";
        let info = parse_doc(doc);
        assert!(info.title.is_none());
    }

    #[test]
    fn heading_splits() {
        let doc = "# Title\n\nIntro text.\n\n## Section One\n\nContent one.\n\n## Section Two\n\nContent two.";
        let chunks = chunk_doc(doc);
        assert_eq!(chunks.len(), 3, "chunks: {chunks:?}");
        assert!(chunks[0].section_title.is_none());
        assert_eq!(chunks[1].section_title.as_deref(), Some("Section One"));
        assert_eq!(chunks[2].section_title.as_deref(), Some("Section Two"));
    }

    #[test]
    fn short_doc_single_chunk() {
        let doc = "# Short\n\nJust a short doc.";
        let chunks = chunk_doc(doc);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn empty_doc() {
        let chunks = chunk_doc("");
        assert!(chunks.is_empty());
        let chunks2 = chunk_doc("   \n  \n  ");
        assert!(chunks2.is_empty());
    }

    #[test]
    fn size_limit_splits() {
        // Create a doc that exceeds CHUNK_SIZE_LIMIT without headings
        let line = "A".repeat(100) + "\n";
        let doc = line.repeat(30); // 30 * 101 = 3030 chars
        let chunks = chunk_doc(&doc);
        assert!(chunks.len() >= 2, "long doc should be split into multiple chunks, got {}", chunks.len());
    }

    #[test]
    fn extract_examples_rust() {
        let doc = "Creates a new runtime.\n\n# Examples\n\n```rust\nlet rt = Runtime::new().unwrap();\nrt.block_on(async { });\n```\n\nSome more text.";
        let examples = extract_code_examples(doc);
        assert_eq!(examples.len(), 1);
        assert_eq!(examples[0].language.as_deref(), Some("rust"));
        assert!(examples[0].code.contains("Runtime::new()"));
    }

    #[test]
    fn extract_examples_multiple() {
        let doc = "```rust\nfirst();\n```\n\nMiddle text.\n\n```\nsecond();\n```";
        let examples = extract_code_examples(doc);
        assert_eq!(examples.len(), 2);
        assert_eq!(examples[0].language.as_deref(), Some("rust"));
        assert!(examples[1].language.is_none());
    }

    #[test]
    fn extract_examples_with_doc_prefix() {
        let doc = "/// Some doc\n/// ```rust\n/// let x = 1;\n/// ```";
        let examples = extract_code_examples(doc);
        assert_eq!(examples.len(), 1);
        assert!(examples[0].code.contains("let x = 1;"));
    }

    #[test]
    fn extract_examples_no_blocks() {
        let doc = "Just a regular doc comment with no code blocks.";
        let examples = extract_code_examples(doc);
        assert!(examples.is_empty());
    }
}
