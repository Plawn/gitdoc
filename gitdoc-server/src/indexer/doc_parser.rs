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
}
