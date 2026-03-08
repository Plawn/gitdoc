use std::sync::atomic::{AtomicBool, Ordering};

use mcp_framework::CapabilityFilter;
use rmcp::model::Tool;

use crate::config::McpMode;

pub const SIMPLE_TOOLS: &[&str] = &[
    "ping",
    "list_repos",
    "register_repo",
    "index_repo",
    "get_repo_overview",
    "ask",
    "conversation_reset",
    "architect_advise",
    "compare_libs",
    "set_mode",
    "get_cheatsheet",
];

pub struct ModeFilter {
    is_granular: AtomicBool,
}

impl ModeFilter {
    pub fn new(mode: McpMode) -> Self {
        Self {
            is_granular: AtomicBool::new(mode == McpMode::Granular),
        }
    }

    pub fn set_granular(&self, value: bool) {
        self.is_granular.store(value, Ordering::SeqCst);
    }

    pub fn is_granular(&self) -> bool {
        self.is_granular.load(Ordering::SeqCst)
    }
}

impl CapabilityFilter for ModeFilter {
    fn filter_tools(
        &self,
        tools: Vec<Tool>,
        _token: Option<&mcp_framework::auth::StoredToken>,
    ) -> Vec<Tool> {
        if self.is_granular() {
            tools
        } else {
            tools
                .into_iter()
                .filter(|t| SIMPLE_TOOLS.contains(&t.name.as_ref()))
                .collect()
        }
    }
}
