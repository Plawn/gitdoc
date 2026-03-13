use super::proto;

// ---------------------------------------------------------------------------
// Repos
// ---------------------------------------------------------------------------

impl From<crate::db::RepoRow> for proto::Repo {
    fn from(v: crate::db::RepoRow) -> Self {
        Self {
            id: v.id,
            name: v.name,
            url: v.url.unwrap_or_default(),
            created_at: v.created_at.to_rfc3339(),
        }
    }
}

impl From<crate::db::RepoSummaryRow> for proto::RepoSummary {
    fn from(v: crate::db::RepoSummaryRow) -> Self {
        Self {
            id: v.id,
            name: v.name,
            url: v.url.unwrap_or_default(),
            created_at: v.created_at.to_rfc3339(),
            snapshot_count: v.snapshot_count,
            latest_snapshot_label: v.latest_snapshot_label.unwrap_or_default(),
            latest_snapshot_commit: v.latest_snapshot_commit.unwrap_or_default(),
            latest_snapshot_indexed_at: v
                .latest_snapshot_indexed_at
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default(),
        }
    }
}

impl From<crate::db::SnapshotRow> for proto::Snapshot {
    fn from(v: crate::db::SnapshotRow) -> Self {
        Self {
            id: v.id,
            repo_id: v.repo_id,
            commit_sha: v.commit_sha,
            label: v.label.unwrap_or_default(),
            indexed_at: v.indexed_at.to_rfc3339(),
            status: v.status,
            stats: v.stats.unwrap_or_default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Docs
// ---------------------------------------------------------------------------

impl From<crate::db::DocRow> for proto::Doc {
    fn from(v: crate::db::DocRow) -> Self {
        Self {
            id: v.id,
            file_path: v.file_path,
            title: v.title.unwrap_or_default(),
        }
    }
}

impl From<crate::db::DocContent> for proto::DocContent {
    fn from(v: crate::db::DocContent) -> Self {
        Self {
            id: v.id,
            file_path: v.file_path,
            title: v.title.unwrap_or_default(),
            content: v.content.unwrap_or_default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Symbols
// ---------------------------------------------------------------------------

impl From<crate::db::SymbolRow> for proto::Symbol {
    fn from(v: crate::db::SymbolRow) -> Self {
        Self {
            id: v.id,
            name: v.name,
            qualified_name: v.qualified_name,
            kind: v.kind,
            visibility: v.visibility,
            file_path: v.file_path,
            line_start: v.line_start,
            line_end: v.line_end,
            signature: v.signature,
            doc_comment: v.doc_comment.unwrap_or_default(),
            parent_id: v.parent_id.unwrap_or(0),
        }
    }
}

impl From<crate::db::SymbolDetail> for proto::SymbolDetail {
    fn from(v: crate::db::SymbolDetail) -> Self {
        Self {
            id: v.id,
            name: v.name,
            qualified_name: v.qualified_name,
            kind: v.kind,
            visibility: v.visibility,
            file_path: v.file_path,
            line_start: v.line_start,
            line_end: v.line_end,
            signature: v.signature,
            doc_comment: v.doc_comment.unwrap_or_default(),
            body: v.body,
            parent_id: v.parent_id.unwrap_or(0),
            children_count: v.children_count,
        }
    }
}

// api-types SymbolRow (used by RefWithSymbol in api-types responses)
impl From<gitdoc_api_types::responses::SymbolRow> for proto::Symbol {
    fn from(v: gitdoc_api_types::responses::SymbolRow) -> Self {
        Self {
            id: v.id,
            name: v.name,
            qualified_name: v.qualified_name,
            kind: v.kind,
            visibility: v.visibility,
            file_path: v.file_path,
            line_start: v.line_start,
            line_end: v.line_end,
            signature: v.signature,
            doc_comment: v.doc_comment.unwrap_or_default(),
            parent_id: v.parent_id.unwrap_or(0),
        }
    }
}

// ---------------------------------------------------------------------------
// References
// ---------------------------------------------------------------------------

impl From<crate::db::RefWithSymbol> for proto::RefWithSymbol {
    fn from(v: crate::db::RefWithSymbol) -> Self {
        Self {
            ref_kind: v.ref_kind,
            symbol: Some(v.symbol.into()),
        }
    }
}

impl From<gitdoc_api_types::responses::RefWithSymbol> for proto::RefWithSymbol {
    fn from(v: gitdoc_api_types::responses::RefWithSymbol) -> Self {
        Self {
            ref_kind: v.ref_kind,
            symbol: Some(v.symbol.into()),
        }
    }
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

impl From<crate::search::DocSearchResult> for proto::DocSearchResult {
    fn from(v: crate::search::DocSearchResult) -> Self {
        Self {
            file_path: v.file_path,
            title: v.title,
            snippets: v.snippets,
        }
    }
}

impl From<crate::search::SymbolSearchResult> for proto::SymbolSearchResult {
    fn from(v: crate::search::SymbolSearchResult) -> Self {
        Self {
            symbol_id: v.symbol_id,
            name: v.name,
            qualified_name: v.qualified_name,
            kind: v.kind,
            visibility: v.visibility,
            signature: v.signature,
            doc_comment: v.doc_comment.unwrap_or_default(),
            file_path: v.file_path,
            score: v.score,
        }
    }
}

// ---------------------------------------------------------------------------
// Diff
// ---------------------------------------------------------------------------

impl From<gitdoc_api_types::responses::DiffSymbolEntry> for proto::DiffSymbolEntry {
    fn from(v: gitdoc_api_types::responses::DiffSymbolEntry) -> Self {
        Self {
            name: v.name,
            qualified_name: v.qualified_name,
            kind: v.kind,
            visibility: v.visibility,
            file_path: v.file_path,
            signature: v.signature,
            body: v.body.unwrap_or_default(),
        }
    }
}

impl From<gitdoc_api_types::responses::DiffSigVis> for proto::DiffSigVis {
    fn from(v: gitdoc_api_types::responses::DiffSigVis) -> Self {
        Self {
            signature: v.signature,
            visibility: v.visibility,
            body: v.body.unwrap_or_default(),
        }
    }
}

impl From<gitdoc_api_types::responses::ModifiedSymbol> for proto::ModifiedSymbol {
    fn from(v: gitdoc_api_types::responses::ModifiedSymbol) -> Self {
        Self {
            qualified_name: v.qualified_name,
            kind: v.kind,
            changes: v.changes,
            from: Some(v.from.into()),
            to: Some(v.to.into()),
        }
    }
}

impl From<gitdoc_api_types::responses::DiffSummary> for proto::DiffSummary {
    fn from(v: gitdoc_api_types::responses::DiffSummary) -> Self {
        Self {
            added: v.added as u64,
            removed: v.removed as u64,
            modified: v.modified as u64,
        }
    }
}

// ---------------------------------------------------------------------------
// Overview
// ---------------------------------------------------------------------------

impl From<gitdoc_api_types::responses::OverviewSymbol> for proto::OverviewSymbol {
    fn from(v: gitdoc_api_types::responses::OverviewSymbol) -> Self {
        Self {
            id: v.id,
            name: v.name,
            qualified_name: v.qualified_name,
            kind: v.kind,
            visibility: v.visibility,
            file_path: v.file_path,
            signature: v.signature,
            doc_comment: v.doc_comment.unwrap_or_default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

impl From<gitdoc_api_types::responses::PublicApiMethod> for proto::PublicApiMethod {
    fn from(v: gitdoc_api_types::responses::PublicApiMethod) -> Self {
        Self {
            id: v.id,
            name: v.name,
            signature: v.signature,
            doc_comment: v.doc_comment.unwrap_or_default(),
        }
    }
}

impl From<gitdoc_api_types::responses::PublicApiEntry> for proto::PublicApiEntry {
    fn from(v: gitdoc_api_types::responses::PublicApiEntry) -> Self {
        Self {
            id: v.id,
            name: v.name,
            qualified_name: v.qualified_name,
            kind: v.kind,
            visibility: v.visibility,
            file_path: v.file_path,
            signature: v.signature,
            doc_comment: v.doc_comment.unwrap_or_default(),
            methods: v.methods.into_iter().map(Into::into).collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// Module tree
// ---------------------------------------------------------------------------

impl From<gitdoc_api_types::responses::ModuleTreeSymbol> for proto::ModuleTreeSymbol {
    fn from(v: gitdoc_api_types::responses::ModuleTreeSymbol) -> Self {
        Self {
            name: v.name,
            kind: v.kind,
            signature: v.signature,
        }
    }
}

impl From<gitdoc_api_types::responses::ModuleTreeNode> for proto::ModuleTreeNode {
    fn from(v: gitdoc_api_types::responses::ModuleTreeNode) -> Self {
        Self {
            name: v.name,
            path: v.path,
            doc_comment: v.doc_comment.unwrap_or_default(),
            public_items: v.public_items,
            children: v.children.into_iter().map(Into::into).collect(),
            symbols: v.symbols.into_iter().map(Into::into).collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// Type context / Examples
// ---------------------------------------------------------------------------

impl From<gitdoc_api_types::responses::UsedBy> for proto::UsedBy {
    fn from(v: gitdoc_api_types::responses::UsedBy) -> Self {
        Self {
            callers: v
                .callers
                .into_iter()
                .map(|r| proto::RefWithSymbol {
                    ref_kind: r.ref_kind,
                    symbol: Some(r.symbol.into()),
                })
                .collect(),
            type_users: v
                .type_users
                .into_iter()
                .map(|r| proto::RefWithSymbol {
                    ref_kind: r.ref_kind,
                    symbol: Some(r.symbol.into()),
                })
                .collect(),
        }
    }
}

impl From<gitdoc_api_types::responses::DependsOn> for proto::DependsOn {
    fn from(v: gitdoc_api_types::responses::DependsOn) -> Self {
        Self {
            types: v
                .types
                .into_iter()
                .map(|r| proto::RefWithSymbol {
                    ref_kind: r.ref_kind,
                    symbol: Some(r.symbol.into()),
                })
                .collect(),
            calls: v
                .calls
                .into_iter()
                .map(|r| proto::RefWithSymbol {
                    ref_kind: r.ref_kind,
                    symbol: Some(r.symbol.into()),
                })
                .collect(),
        }
    }
}

impl From<gitdoc_api_types::responses::CodeExample> for proto::CodeExample {
    fn from(v: gitdoc_api_types::responses::CodeExample) -> Self {
        Self {
            language: v.language.unwrap_or_default(),
            code: v.code,
        }
    }
}

// ---------------------------------------------------------------------------
// Explain
// ---------------------------------------------------------------------------

impl From<gitdoc_api_types::responses::MethodInfo> for proto::MethodInfo {
    fn from(v: gitdoc_api_types::responses::MethodInfo) -> Self {
        Self {
            name: v.name,
            signature: v.signature,
        }
    }
}

impl From<gitdoc_api_types::responses::RelevantSymbol> for proto::RelevantSymbol {
    fn from(v: gitdoc_api_types::responses::RelevantSymbol) -> Self {
        Self {
            id: v.id,
            name: v.name,
            qualified_name: v.qualified_name,
            kind: v.kind,
            signature: v.signature,
            doc_comment: v.doc_comment.unwrap_or_default(),
            file_path: v.file_path,
            score: v.score,
            methods: v.methods.into_iter().map(Into::into).collect(),
            traits: v.traits,
        }
    }
}

impl From<gitdoc_api_types::responses::RelevantDoc> for proto::RelevantDoc {
    fn from(v: gitdoc_api_types::responses::RelevantDoc) -> Self {
        Self {
            file_path: v.file_path,
            title: v.title.unwrap_or_default(),
            snippet: v.snippet,
            score: v.score,
        }
    }
}

// ---------------------------------------------------------------------------
// Conversation
// ---------------------------------------------------------------------------

impl From<gitdoc_api_types::responses::SourceRef> for proto::SourceRef {
    fn from(v: gitdoc_api_types::responses::SourceRef) -> Self {
        Self {
            kind: v.kind,
            name: v.name,
            file_path: v.file_path,
            symbol_id: v.symbol_id.unwrap_or(0),
        }
    }
}

impl From<gitdoc_api_types::responses::ConversationResponse> for proto::ConverseResponse {
    fn from(v: gitdoc_api_types::responses::ConversationResponse) -> Self {
        Self {
            conversation_id: v.conversation_id,
            answer: v.answer,
            sources: v.sources.into_iter().map(Into::into).collect(),
            turn_index: v.turn_index,
        }
    }
}

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------

impl From<crate::db::SummaryRow> for proto::SummaryRow {
    fn from(v: crate::db::SummaryRow) -> Self {
        Self {
            id: v.id,
            snapshot_id: v.snapshot_id,
            scope: v.scope,
            content: v.content,
            model: v.model,
            created_at: v.created_at.to_rfc3339(),
        }
    }
}

// ---------------------------------------------------------------------------
// Cheatsheet
// ---------------------------------------------------------------------------

impl From<crate::db::CheatsheetRow> for proto::GetCheatsheetResponse {
    fn from(v: crate::db::CheatsheetRow) -> Self {
        Self {
            repo_id: v.repo_id,
            content: v.content,
            model: v.model,
            updated_at: v.updated_at.to_rfc3339(),
        }
    }
}

impl From<crate::db::CheatsheetPatchMeta> for proto::CheatsheetPatchMeta {
    fn from(v: crate::db::CheatsheetPatchMeta) -> Self {
        Self {
            id: v.id,
            repo_id: v.repo_id,
            snapshot_id: v.snapshot_id.unwrap_or(0),
            change_summary: v.change_summary,
            trigger: v.trigger,
            model: v.model,
            created_at: v.created_at.to_rfc3339(),
        }
    }
}

impl From<crate::db::CheatsheetPatchRow> for proto::CheatsheetPatchRow {
    fn from(v: crate::db::CheatsheetPatchRow) -> Self {
        Self {
            id: v.id,
            repo_id: v.repo_id,
            snapshot_id: v.snapshot_id.unwrap_or(0),
            prev_content: v.prev_content,
            new_content: v.new_content,
            change_summary: v.change_summary,
            trigger: v.trigger,
            model: v.model,
            created_at: v.created_at.to_rfc3339(),
        }
    }
}

// ---------------------------------------------------------------------------
// Architect — Libs
// ---------------------------------------------------------------------------

impl From<crate::db::LibProfileSummary> for proto::LibProfileSummary {
    fn from(v: crate::db::LibProfileSummary) -> Self {
        Self {
            id: v.id,
            name: v.name,
            category: v.category,
            version_hint: v.version_hint,
            source: v.source,
            updated_at: v.updated_at.to_rfc3339(),
        }
    }
}

impl From<crate::db::LibProfileRow> for proto::LibProfileRow {
    fn from(v: crate::db::LibProfileRow) -> Self {
        Self {
            id: v.id,
            name: v.name,
            repo_id: v.repo_id.unwrap_or_default(),
            category: v.category,
            version_hint: v.version_hint,
            profile: v.profile,
            source: v.source,
            model: v.model,
            created_at: v.created_at.to_rfc3339(),
            updated_at: v.updated_at.to_rfc3339(),
        }
    }
}

// ---------------------------------------------------------------------------
// Architect — Rules
// ---------------------------------------------------------------------------

impl From<crate::db::StackRuleRow> for proto::StackRuleRow {
    fn from(v: crate::db::StackRuleRow) -> Self {
        Self {
            id: v.id,
            rule_type: v.rule_type,
            subject: v.subject,
            content: v.content,
            lib_profile_id: v.lib_profile_id.unwrap_or_default(),
            priority: v.priority,
            created_at: v.created_at.to_rfc3339(),
            updated_at: v.updated_at.to_rfc3339(),
        }
    }
}

// ---------------------------------------------------------------------------
// Architect — Advise / Compare
// ---------------------------------------------------------------------------

impl From<crate::db::ArchitectSearchResult> for proto::ArchitectSearchResult {
    fn from(v: crate::db::ArchitectSearchResult) -> Self {
        Self {
            id: v.id,
            kind: v.kind,
            text: v.text,
            score: v.score,
        }
    }
}

// ---------------------------------------------------------------------------
// Architect — Projects
// ---------------------------------------------------------------------------

impl From<crate::db::ProjectProfileRow> for proto::ProjectProfileRow {
    fn from(v: crate::db::ProjectProfileRow) -> Self {
        Self {
            id: v.id,
            repo_id: v.repo_id.unwrap_or_default(),
            name: v.name,
            description: v.description,
            stack: serde_json::to_string(&v.stack).unwrap_or_default(),
            constraints: v.constraints,
            code_style: v.code_style,
            created_at: v.created_at.to_rfc3339(),
            updated_at: v.updated_at.to_rfc3339(),
        }
    }
}

impl From<crate::db::ProjectProfileSummary> for proto::ProjectProfileSummary {
    fn from(v: crate::db::ProjectProfileSummary) -> Self {
        Self {
            id: v.id,
            name: v.name,
            repo_id: v.repo_id.unwrap_or_default(),
            updated_at: v.updated_at.to_rfc3339(),
        }
    }
}

// ---------------------------------------------------------------------------
// Architect — Decisions
// ---------------------------------------------------------------------------

impl From<crate::db::ArchDecisionRow> for proto::ArchDecisionRow {
    fn from(v: crate::db::ArchDecisionRow) -> Self {
        Self {
            id: v.id,
            project_profile_id: v.project_profile_id.unwrap_or_default(),
            title: v.title,
            context: v.context,
            choice: v.choice,
            alternatives: v.alternatives,
            reasoning: v.reasoning,
            outcome: v.outcome.unwrap_or_default(),
            status: v.status,
            created_at: v.created_at.to_rfc3339(),
            updated_at: v.updated_at.to_rfc3339(),
        }
    }
}

// ---------------------------------------------------------------------------
// Architect — Patterns
// ---------------------------------------------------------------------------

impl From<crate::db::ArchPatternRow> for proto::ArchPatternRow {
    fn from(v: crate::db::ArchPatternRow) -> Self {
        Self {
            id: v.id,
            name: v.name,
            category: v.category,
            description: v.description,
            libs_involved: v.libs_involved,
            pattern_text: v.pattern_text,
            source: v.source,
            created_at: v.created_at.to_rfc3339(),
            updated_at: v.updated_at.to_rfc3339(),
        }
    }
}

// ---------------------------------------------------------------------------
// GC Stats
// ---------------------------------------------------------------------------

impl From<crate::db::GcStats> for proto::GarbageCollectResponse {
    fn from(v: crate::db::GcStats) -> Self {
        Self {
            files_removed: v.files_removed as i64,
            symbols_removed: v.symbols_removed as i64,
            docs_removed: v.docs_removed as i64,
            refs_removed: v.refs_removed as i64,
            embeddings_removed: v.embeddings_removed as i64,
        }
    }
}

// ---------------------------------------------------------------------------
// IndexResult
// ---------------------------------------------------------------------------

impl From<crate::indexer::pipeline::IndexResult> for proto::IndexRepoResponse {
    fn from(v: crate::indexer::pipeline::IndexResult) -> Self {
        Self {
            snapshot_id: v.snapshot_id,
            files_scanned: v.files_scanned,
            docs_count: v.docs_count,
            symbols_count: v.symbols_count,
            refs_count: v.refs_count as u64,
            embeddings_count: v.embeddings_count as u64,
            duration_ms: v.duration_ms,
        }
    }
}
