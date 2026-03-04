pub struct DocInfo {
    pub title: Option<String>,
}

/// Extract the title from a markdown file (first `# heading`).
pub fn parse_doc(content: &str) -> DocInfo {
    let title = content.lines().find_map(|line| {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("# ") {
            Some(rest.trim().to_string())
        } else {
            None
        }
    });
    DocInfo { title }
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
}
