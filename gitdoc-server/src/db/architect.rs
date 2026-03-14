use anyhow::Result;
use super::types::{
    LibProfileRow, LibProfileSummary, StackRuleRow, ArchitectSearchResult, ArchitectResultKind,
    ProjectProfileRow, ProjectProfileSummary, ArchDecisionRow, ArchPatternRow,
};

impl super::Database {
    // ==================== Lib Profiles ====================

    pub async fn get_lib_profile(&self, id: &str) -> Result<Option<LibProfileRow>> {
        let row = sqlx::query_as::<_, LibProfileRow>(
            "SELECT id, name, repo_id, category, version_hint, profile, source, model, created_at, updated_at
             FROM lib_profiles WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn list_lib_profiles(&self, category: Option<&str>) -> Result<Vec<LibProfileSummary>> {
        let rows = if let Some(cat) = category {
            sqlx::query_as::<_, LibProfileSummary>(
                "SELECT id, name, category, version_hint, source, updated_at
                 FROM lib_profiles WHERE category = $1
                 ORDER BY name",
            )
            .bind(cat)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, LibProfileSummary>(
                "SELECT id, name, category, version_hint, source, updated_at
                 FROM lib_profiles ORDER BY name",
            )
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows)
    }

    pub async fn upsert_lib_profile(
        &self,
        id: &str,
        name: &str,
        repo_id: Option<&str>,
        category: &str,
        version_hint: &str,
        profile: &str,
        source: &str,
        model: &str,
        embedding: Option<pgvector::Vector>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO lib_profiles (id, name, repo_id, category, version_hint, profile, profile_embedding, source, model)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (id)
             DO UPDATE SET name = EXCLUDED.name,
                           repo_id = EXCLUDED.repo_id,
                           category = EXCLUDED.category,
                           version_hint = EXCLUDED.version_hint,
                           profile = EXCLUDED.profile,
                           profile_embedding = EXCLUDED.profile_embedding,
                           source = EXCLUDED.source,
                           model = EXCLUDED.model,
                           updated_at = NOW()",
        )
        .bind(id)
        .bind(name)
        .bind(repo_id)
        .bind(category)
        .bind(version_hint)
        .bind(profile)
        .bind(embedding)
        .bind(source)
        .bind(model)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_lib_profile(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM lib_profiles WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // ==================== Stack Rules ====================

    pub async fn get_stack_rule(&self, id: i64) -> Result<Option<StackRuleRow>> {
        let row = sqlx::query_as::<_, StackRuleRow>(
            "SELECT id, rule_type, subject, content, lib_profile_id, priority, created_at, updated_at
             FROM stack_rules WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn list_stack_rules(
        &self,
        rule_type: Option<&str>,
        subject: Option<&str>,
    ) -> Result<Vec<StackRuleRow>> {
        let mut query = String::from(
            "SELECT id, rule_type, subject, content, lib_profile_id, priority, created_at, updated_at
             FROM stack_rules WHERE 1=1"
        );
        let mut param_idx = 1;
        let mut binds: Vec<String> = Vec::new();

        if let Some(rt) = rule_type {
            query.push_str(&format!(" AND rule_type = ${param_idx}"));
            param_idx += 1;
            binds.push(rt.to_string());
        }
        if let Some(sub) = subject {
            query.push_str(&format!(" AND subject = ${param_idx}"));
            binds.push(sub.to_string());
        }
        query.push_str(" ORDER BY priority DESC, created_at DESC");

        let mut q = sqlx::query_as::<_, StackRuleRow>(&query);
        for b in &binds {
            q = q.bind(b);
        }
        let rows = q.fetch_all(&self.pool).await?;
        Ok(rows)
    }

    pub async fn upsert_stack_rule(
        &self,
        id: Option<i64>,
        rule_type: &str,
        subject: &str,
        content: &str,
        lib_profile_id: Option<&str>,
        priority: i32,
        embedding: Option<pgvector::Vector>,
    ) -> Result<i64> {
        if let Some(existing_id) = id {
            sqlx::query(
                "UPDATE stack_rules SET rule_type = $2, subject = $3, content = $4,
                 lib_profile_id = $5, priority = $6, content_embedding = $7, updated_at = NOW()
                 WHERE id = $1",
            )
            .bind(existing_id)
            .bind(rule_type)
            .bind(subject)
            .bind(content)
            .bind(lib_profile_id)
            .bind(priority)
            .bind(embedding)
            .execute(&self.pool)
            .await?;
            Ok(existing_id)
        } else {
            let (new_id,): (i64,) = sqlx::query_as(
                "INSERT INTO stack_rules (rule_type, subject, content, lib_profile_id, priority, content_embedding)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 RETURNING id",
            )
            .bind(rule_type)
            .bind(subject)
            .bind(content)
            .bind(lib_profile_id)
            .bind(priority)
            .bind(embedding)
            .fetch_one(&self.pool)
            .await?;
            Ok(new_id)
        }
    }

    pub async fn delete_stack_rule(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM stack_rules WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // ==================== Project Profiles ====================

    pub async fn get_project_profile(&self, id: &str) -> Result<Option<ProjectProfileRow>> {
        let row = sqlx::query_as::<_, ProjectProfileRow>(
            "SELECT id, repo_id, name, description, stack, constraints, code_style, created_at, updated_at
             FROM project_profiles WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn list_project_profiles(&self) -> Result<Vec<ProjectProfileSummary>> {
        let rows = sqlx::query_as::<_, ProjectProfileSummary>(
            "SELECT id, name, repo_id, updated_at
             FROM project_profiles ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn upsert_project_profile(
        &self,
        id: &str,
        repo_id: Option<&str>,
        name: &str,
        description: &str,
        stack: &serde_json::Value,
        constraints: &str,
        code_style: &str,
        embedding: Option<pgvector::Vector>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO project_profiles (id, repo_id, name, description, stack, constraints, code_style, content_embedding)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (id)
             DO UPDATE SET repo_id = EXCLUDED.repo_id,
                           name = EXCLUDED.name,
                           description = EXCLUDED.description,
                           stack = EXCLUDED.stack,
                           constraints = EXCLUDED.constraints,
                           code_style = EXCLUDED.code_style,
                           content_embedding = EXCLUDED.content_embedding,
                           updated_at = NOW()",
        )
        .bind(id)
        .bind(repo_id)
        .bind(name)
        .bind(description)
        .bind(stack)
        .bind(constraints)
        .bind(code_style)
        .bind(embedding)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_project_profile(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM project_profiles WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // ==================== Architecture Decisions ====================

    pub async fn get_arch_decision(&self, id: i64) -> Result<Option<ArchDecisionRow>> {
        let row = sqlx::query_as::<_, ArchDecisionRow>(
            "SELECT id, project_profile_id, title, context, choice, alternatives, reasoning, outcome, status, created_at, updated_at
             FROM arch_decisions WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn list_arch_decisions(
        &self,
        project_profile_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<ArchDecisionRow>> {
        let mut query = String::from(
            "SELECT id, project_profile_id, title, context, choice, alternatives, reasoning, outcome, status, created_at, updated_at
             FROM arch_decisions WHERE 1=1"
        );
        let mut param_idx = 1;
        let mut binds: Vec<String> = Vec::new();

        if let Some(pid) = project_profile_id {
            query.push_str(&format!(" AND project_profile_id = ${param_idx}"));
            param_idx += 1;
            binds.push(pid.to_string());
        }
        if let Some(s) = status {
            query.push_str(&format!(" AND status = ${param_idx}"));
            binds.push(s.to_string());
        }
        query.push_str(" ORDER BY created_at DESC");

        let mut q = sqlx::query_as::<_, ArchDecisionRow>(&query);
        for b in &binds {
            q = q.bind(b);
        }
        let rows = q.fetch_all(&self.pool).await?;
        Ok(rows)
    }

    pub async fn create_arch_decision(
        &self,
        project_profile_id: Option<&str>,
        title: &str,
        context: &str,
        choice: &str,
        alternatives: &str,
        reasoning: &str,
        embedding: Option<pgvector::Vector>,
    ) -> Result<i64> {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO arch_decisions (project_profile_id, title, context, choice, alternatives, reasoning, content_embedding)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING id",
        )
        .bind(project_profile_id)
        .bind(title)
        .bind(context)
        .bind(choice)
        .bind(alternatives)
        .bind(reasoning)
        .bind(embedding)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn update_arch_decision(
        &self,
        id: i64,
        outcome: Option<&str>,
        status: Option<&str>,
        embedding: Option<pgvector::Vector>,
    ) -> Result<bool> {
        // Use COALESCE-style update: only overwrite if new value is provided
        let result = sqlx::query(
            "UPDATE arch_decisions SET
                outcome = COALESCE($2, outcome),
                status = COALESCE($3, status),
                content_embedding = COALESCE($4, content_embedding),
                updated_at = NOW()
             WHERE id = $1",
        )
        .bind(id)
        .bind(outcome)
        .bind(status)
        .bind(embedding)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_arch_decision(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM arch_decisions WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // ==================== Architecture Patterns ====================

    pub async fn get_arch_pattern(&self, id: i64) -> Result<Option<ArchPatternRow>> {
        let row = sqlx::query_as::<_, ArchPatternRow>(
            "SELECT id, name, category, description, libs_involved, pattern_text, source, created_at, updated_at
             FROM arch_patterns WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn list_arch_patterns(
        &self,
        category: Option<&str>,
    ) -> Result<Vec<ArchPatternRow>> {
        if let Some(cat) = category {
            let rows = sqlx::query_as::<_, ArchPatternRow>(
                "SELECT id, name, category, description, libs_involved, pattern_text, source, created_at, updated_at
                 FROM arch_patterns WHERE category = $1
                 ORDER BY name",
            )
            .bind(cat)
            .fetch_all(&self.pool)
            .await?;
            Ok(rows)
        } else {
            let rows = sqlx::query_as::<_, ArchPatternRow>(
                "SELECT id, name, category, description, libs_involved, pattern_text, source, created_at, updated_at
                 FROM arch_patterns ORDER BY name",
            )
            .fetch_all(&self.pool)
            .await?;
            Ok(rows)
        }
    }

    pub async fn create_arch_pattern(
        &self,
        name: &str,
        category: &str,
        description: &str,
        libs_involved: &[String],
        pattern_text: &str,
        source: &str,
        embedding: Option<pgvector::Vector>,
    ) -> Result<i64> {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO arch_patterns (name, category, description, libs_involved, pattern_text, source, content_embedding)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING id",
        )
        .bind(name)
        .bind(category)
        .bind(description)
        .bind(libs_involved)
        .bind(pattern_text)
        .bind(source)
        .bind(embedding)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn delete_arch_pattern(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM arch_patterns WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // ==================== Vector Search ====================

    pub async fn search_architect_by_vector(
        &self,
        query_vec: &pgvector::Vector,
        limit: i64,
    ) -> Result<Vec<ArchitectSearchResult>> {
        let rows = sqlx::query_as::<_, (String, String, String, f64)>(
            "SELECT id, kind, text, score FROM (
                SELECT id::TEXT, 'lib_profile'::TEXT AS kind, profile AS text,
                       1.0 - (profile_embedding <=> $1) AS score
                FROM lib_profiles
                WHERE profile_embedding IS NOT NULL
                UNION ALL
                SELECT id::TEXT, 'stack_rule'::TEXT AS kind, content AS text,
                       1.0 - (content_embedding <=> $1) AS score
                FROM stack_rules
                WHERE content_embedding IS NOT NULL
                UNION ALL
                SELECT repo_id::TEXT, 'cheatsheet'::TEXT AS kind, content AS text,
                       1.0 - (content_embedding <=> $1) AS score
                FROM repo_cheatsheets
                WHERE content_embedding IS NOT NULL
                UNION ALL
                SELECT id::TEXT, 'project_profile'::TEXT AS kind,
                       name || ': ' || description || ' | stack: ' || stack::TEXT || ' | constraints: ' || constraints AS text,
                       1.0 - (content_embedding <=> $1) AS score
                FROM project_profiles
                WHERE content_embedding IS NOT NULL
                UNION ALL
                SELECT id::TEXT, 'decision'::TEXT AS kind,
                       title || ': ' || choice || ' (status: ' || status || ')' AS text,
                       1.0 - (content_embedding <=> $1) AS score
                FROM arch_decisions
                WHERE content_embedding IS NOT NULL
                UNION ALL
                SELECT id::TEXT, 'pattern'::TEXT AS kind,
                       name || ': ' || description AS text,
                       1.0 - (content_embedding <=> $1) AS score
                FROM arch_patterns
                WHERE content_embedding IS NOT NULL
            ) combined
            ORDER BY score DESC
            LIMIT $2",
        )
        .bind(query_vec)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|(id, kind, text, score)| {
                match kind.parse::<ArchitectResultKind>() {
                    Ok(k) => Some(ArchitectSearchResult { id, kind: k, text, score }),
                    Err(e) => {
                        tracing::warn!(kind, error = %e, "unknown architect result kind from DB, skipping");
                        None
                    }
                }
            })
            .collect())
    }
}
