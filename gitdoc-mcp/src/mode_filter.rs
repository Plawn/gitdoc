use mcp_framework::CapabilityFilter;
use rmcp::model::Tool;

use crate::config::McpMode;

const SIMPLE_TOOLS: &[&str] = &[
    "ping",
    "list_repos",
    "register_repo",
    "index_repo",
    "get_repo_overview",
    "ask",
    "conversation_reset",
];

pub struct ModeFilter {
    pub mode: McpMode,
}

impl CapabilityFilter for ModeFilter {
    fn filter_tools(
        &self,
        tools: Vec<Tool>,
        _token: Option<&mcp_framework::auth::StoredToken>,
    ) -> Vec<Tool> {
        match self.mode {
            McpMode::Granular => tools,
            McpMode::Simple => tools
                .into_iter()
                .filter(|t| SIMPLE_TOOLS.contains(&t.name.as_ref()))
                .collect(),
        }
    }
}
