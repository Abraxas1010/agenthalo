use crate::immutable::WriteMode;
use crate::protocol::NucleusDb;
use crate::sql::schema::{COL_KEY, COL_TYPE, COL_VALUE, TABLE_NAME};
use crate::state::Delta;
use crate::transparency::ct6962::hex_encode;
use crate::typed_value::{infer_from_string, TypeTag, TypedValue};
use crate::vector_index::DistanceMetric;
use chrono::{TimeZone, Utc};
use sqlparser::ast::{
    Assignment, AssignmentTarget, BinaryOperator, CreateTable, Delete, Expr, FunctionArg,
    FunctionArgExpr, Ident, Insert, ObjectName, ObjectNamePart, Query, SelectItem, SetExpr,
    Statement, TableFactor, TableObject, TableWithJoins, Value,
};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlResult {
    Rows {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Ok {
        message: String,
    },
    Error {
        message: String,
    },
}

pub struct SqlExecutor<'a> {
    db: &'a mut NucleusDb,
    pending_writes: Vec<(usize, u64)>,
    committed: bool,
}

impl<'a> SqlExecutor<'a> {
    pub fn new(db: &'a mut NucleusDb) -> Self {
        Self {
            db,
            pending_writes: Vec::new(),
            committed: false,
        }
    }

    pub fn execute(&mut self, sql: &str) -> SqlResult {
        let trimmed = sql.trim();
        if trimmed.is_empty() {
            return SqlResult::Ok {
                message: "No statements".to_string(),
            };
        }

        let statements = split_sql_statements(trimmed);
        if statements.len() > 1 {
            let mut last = SqlResult::Ok {
                message: "No-op".to_string(),
            };
            for stmt in statements {
                last = self.execute(&stmt);
                if matches!(last, SqlResult::Error { .. }) {
                    return last;
                }
            }
            return last;
        }

        if let Some(custom) = self.execute_custom(trimmed) {
            return custom;
        }

        let dialect = GenericDialect {};
        let ast = match Parser::parse_sql(&dialect, trimmed) {
            Ok(stmts) => stmts,
            Err(e) => {
                return SqlResult::Error {
                    message: format!("Parse error: {e}"),
                };
            }
        };

        if ast.is_empty() {
            return SqlResult::Ok {
                message: "No statements".to_string(),
            };
        }

        let mut last = SqlResult::Ok {
            message: "No-op".to_string(),
        };
        for stmt in &ast {
            last = self.execute_statement(stmt);
            if matches!(last, SqlResult::Error { .. }) {
                return last;
            }
        }
        last
    }

    /// Returns true if any COMMIT succeeded during this executor's lifetime.
    pub fn committed(&self) -> bool {
        self.committed
    }

    pub fn pending_writes_len(&self) -> usize {
        self.pending_writes.len()
    }

    pub fn db(&self) -> &NucleusDb {
        self.db
    }

    fn execute_custom(&mut self, sql: &str) -> Option<SqlResult> {
        let normalized = normalize_command(sql);
        match normalized.as_str() {
            "SHOW STATUS" => Some(self.show_status()),
            "SHOW HISTORY" => Some(self.show_history()),
            "COMMIT" => Some(self.flush_commit()),
            "VERIFY" => Some(SqlResult::Error {
                message: "VERIFY requires a quoted key: VERIFY 'my_key'".to_string(),
            }),
            "CHECKPOINT" => Some(SqlResult::Error {
                message: "CHECKPOINT requires a configured snapshot/WAL path in CLI mode"
                    .to_string(),
            }),
            "EXPORT" => Some(self.export_json()),
            "SHOW TYPES" => Some(self.show_types()),
            "SHOW MODE" => Some(SqlResult::Ok {
                message: format!("Write mode: {:?}", self.db.write_mode()),
            }),
            "SET MODE APPEND_ONLY" | "SET MODE APPENDONLY" => {
                self.db.set_append_only();
                Some(SqlResult::Ok {
                    message: "Write mode locked to AppendOnly. INSERT only — UPDATE/DELETE disabled. Monotone seal chain active.".to_string(),
                })
            }
            _ => {
                if normalized.starts_with("SHOW HISTORY ") {
                    return Some(self.show_key_history(sql));
                }
                if normalized.starts_with("VERIFY ") {
                    return Some(self.verify_key(sql));
                }
                None
            }
        }
    }

    fn execute_statement(&mut self, stmt: &Statement) -> SqlResult {
        match stmt {
            Statement::Query(q) => self.execute_select(q),
            Statement::Insert(ins) => self.execute_insert(ins),
            Statement::Commit { .. } => self.flush_commit(),
            Statement::Update {
                table,
                assignments,
                selection,
                ..
            } => self.execute_update(table, assignments, selection.as_ref()),
            Statement::Delete(del) => self.execute_delete(del),
            Statement::CreateTable(ct) => self.execute_create_table(ct),
            _ => SqlResult::Error {
                message: "Unsupported SQL statement".to_string(),
            },
        }
    }

    fn show_status(&self) -> SqlResult {
        let mut rows = vec![
            vec!["backend".to_string(), format!("{:?}", self.db.backend)],
            vec![
                "state_len".to_string(),
                self.db.state.values.len().to_string(),
            ],
            vec!["entries".to_string(), self.db.entries.len().to_string()],
            vec!["key_count".to_string(), self.db.keymap.len().to_string()],
            vec![
                "pending_writes".to_string(),
                self.pending_writes.len().to_string(),
            ],
        ];

        if let Some(sth) = self.db.current_sth() {
            rows.push(vec!["sth_tree_size".to_string(), sth.tree_size.to_string()]);
            rows.push(vec!["sth_root".to_string(), hex_encode(&sth.root_hash)]);
            rows.push(vec![
                "sth_timestamp".to_string(),
                sth.timestamp_unix_secs.to_string(),
            ]);
            rows.push(vec![
                "sth_timestamp_utc".to_string(),
                format_unix_utc(sth.timestamp_unix_secs),
            ]);
        } else {
            rows.push(vec!["sth_tree_size".to_string(), "0".to_string()]);
            rows.push(vec!["sth_root".to_string(), String::new()]);
            rows.push(vec!["sth_timestamp".to_string(), "0".to_string()]);
            rows.push(vec!["sth_timestamp_utc".to_string(), "n/a".to_string()]);
        }

        SqlResult::Rows {
            columns: vec!["field".to_string(), "value".to_string()],
            rows,
        }
    }

    fn show_types(&self) -> SqlResult {
        let mut type_counts = BTreeMap::<String, usize>::new();
        for (key, _idx) in self.db.keymap.all_keys() {
            let tag = self.db.type_map.get(key);
            *type_counts.entry(tag.as_str().to_string()).or_default() += 1;
        }
        let rows: Vec<Vec<String>> = type_counts
            .into_iter()
            .map(|(t, c)| vec![t, c.to_string()])
            .collect();
        SqlResult::Rows {
            columns: vec!["type".to_string(), "count".to_string()],
            rows,
        }
    }

    fn show_history(&self) -> SqlResult {
        let rows = self
            .db
            .entries
            .iter()
            .map(|e| {
                vec![
                    e.height.to_string(),
                    hex_encode(&e.state_root),
                    e.sth.tree_size.to_string(),
                    e.sth.timestamp_unix_secs.to_string(),
                    format_unix_utc(e.sth.timestamp_unix_secs),
                    e.vc_backend_id.clone(),
                    e.witness_signature_algorithm.clone(),
                ]
            })
            .collect();
        SqlResult::Rows {
            columns: vec![
                "height".to_string(),
                "state_root".to_string(),
                "tree_size".to_string(),
                "timestamp_unix".to_string(),
                "timestamp_utc".to_string(),
                "backend".to_string(),
                "witness_algorithm".to_string(),
            ],
            rows,
        }
    }

    fn show_key_history(&self, sql: &str) -> SqlResult {
        let key = match extract_single_quoted_argument(sql) {
            Some(k) => k,
            None => {
                return SqlResult::Error {
                    message: "SHOW HISTORY requires a quoted key: SHOW HISTORY 'my_key'"
                        .to_string(),
                };
            }
        };
        let Some(idx) = self.db.keymap.get(&key) else {
            return SqlResult::Rows {
                columns: vec![
                    "key".to_string(),
                    "index".to_string(),
                    "value".to_string(),
                    "note".to_string(),
                ],
                rows: vec![],
            };
        };
        let pending = self.pending_overlay();
        let value = pending
            .get(&idx)
            .copied()
            .unwrap_or_else(|| self.db.state.values.get(idx).copied().unwrap_or(0));
        SqlResult::Rows {
            columns: vec![
                "key".to_string(),
                "index".to_string(),
                "value".to_string(),
                "note".to_string(),
            ],
            rows: vec![vec![
                key,
                idx.to_string(),
                value.to_string(),
                "per-commit key history is not available in CommitEntry v1".to_string(),
            ]],
        }
    }

    fn verify_key(&self, sql: &str) -> SqlResult {
        if !self.pending_writes.is_empty() {
            return SqlResult::Error {
                message: "VERIFY requires COMMIT first when pending writes exist".to_string(),
            };
        }
        let key = match extract_single_quoted_argument(sql) {
            Some(k) => k,
            None => {
                return SqlResult::Error {
                    message: "VERIFY requires a quoted key: VERIFY 'my_key'".to_string(),
                };
            }
        };
        let Some(idx) = self.db.keymap.get(&key) else {
            return SqlResult::Error {
                message: format!("Unknown key '{key}'"),
            };
        };
        let Some((value, proof, root)) = self.db.query(idx) else {
            return SqlResult::Error {
                message: format!("No value for key '{key}'"),
            };
        };
        let ok = self.db.verify_query(idx, value, &proof, root);
        SqlResult::Rows {
            columns: vec![
                "key".to_string(),
                "index".to_string(),
                "value".to_string(),
                "verified".to_string(),
                "root".to_string(),
            ],
            rows: vec![vec![
                key,
                idx.to_string(),
                value.to_string(),
                ok.to_string(),
                hex_encode(&root),
            ]],
        }
    }

    fn flush_commit(&mut self) -> SqlResult {
        if self.pending_writes.is_empty() {
            return SqlResult::Ok {
                message: "No pending writes".to_string(),
            };
        }

        let mut dedup = BTreeMap::new();
        for (idx, value) in &self.pending_writes {
            dedup.insert(*idx, *value);
        }
        let writes: Vec<(usize, u64)> = dedup.into_iter().collect();
        let delta = Delta::new(writes);
        match self.db.commit(delta, &[]) {
            Ok(entry) => {
                self.pending_writes.clear();
                self.committed = true;
                SqlResult::Ok {
                    message: format!("Committed at height {}", entry.height),
                }
            }
            Err(e) => SqlResult::Error {
                message: format!("Commit failed: {e:?}"),
            },
        }
    }

    fn export_json(&self) -> SqlResult {
        let pending = self.pending_overlay();
        let mut payload = BTreeMap::<String, serde_json::Value>::new();
        for (k, idx) in self.db.keymap.all_keys() {
            let cell = pending
                .get(&idx)
                .copied()
                .unwrap_or_else(|| self.db.state.values.get(idx).copied().unwrap_or(0));
            let tag = self.db.type_map.get(k);
            let blob = self.db.blob_store.get(k);
            let typed = match TypedValue::decode(tag, cell, blob) {
                Ok(v) => v,
                Err(e) => {
                    return SqlResult::Error {
                        message: format!("typed decode failed for key '{k}': {e}"),
                    };
                }
            };
            payload.insert(k.to_string(), typed.to_json_value());
        }
        match serde_json::to_string_pretty(&payload) {
            Ok(json) => SqlResult::Rows {
                columns: vec!["json".to_string()],
                rows: vec![vec![json]],
            },
            Err(e) => SqlResult::Error {
                message: format!("Export failed: {e}"),
            },
        }
    }

    fn execute_create_table(&self, ct: &CreateTable) -> SqlResult {
        if !object_name_is_data(&ct.name) {
            return SqlResult::Error {
                message: format!("Only virtual table '{TABLE_NAME}' is supported"),
            };
        }
        SqlResult::Ok {
            message: format!("Virtual table '{TABLE_NAME}' is available"),
        }
    }

    fn execute_insert(&mut self, ins: &Insert) -> SqlResult {
        if !insert_targets_data(ins) {
            return SqlResult::Error {
                message: format!("Only virtual table '{TABLE_NAME}' is supported"),
            };
        }
        if !ins.assignments.is_empty() {
            return SqlResult::Error {
                message: "INSERT ... SET form is not supported".to_string(),
            };
        }
        let source = match ins.source.as_ref() {
            Some(s) => s,
            None => {
                return SqlResult::Error {
                    message: "INSERT requires VALUES source".to_string(),
                };
            }
        };

        // Try typed insert first, fall back to legacy u64
        match extract_insert_typed(source, &ins.columns) {
            Ok(TypedInsert::Typed(key, typed_val)) => {
                let (idx, cell) = match self.db.put_typed(&key, typed_val.clone()) {
                    Ok(v) => v,
                    Err(e) => {
                        return SqlResult::Error {
                            message: format!("typed insert failed for '{key}': {e}"),
                        };
                    }
                };
                self.pending_writes.push((idx, cell));
                SqlResult::Ok {
                    message: format!(
                        "Queued write: {key}={} [{}] (idx={idx})",
                        typed_val.display_string(),
                        typed_val.tag()
                    ),
                }
            }
            Ok(TypedInsert::LegacyU64(key, value)) => {
                let idx = self.db.keymap.get_or_create(&key);
                self.pending_writes.push((idx, value));
                SqlResult::Ok {
                    message: format!("Queued write: {key}={value} (idx={idx})"),
                }
            }
            Err(e) => SqlResult::Error { message: e },
        }
    }

    fn execute_select(&self, q: &Query) -> SqlResult {
        let Some(select) = q.body.as_select() else {
            return SqlResult::Error {
                message: "Only SELECT queries are supported".to_string(),
            };
        };

        if !select.from.is_empty()
            && (select.from.len() != 1 || !table_with_joins_is_data(&select.from[0]))
        {
            return SqlResult::Error {
                message: format!("Only SELECT ... FROM {TABLE_NAME} is supported"),
            };
        }

        let projection = match resolve_projection(&select.projection) {
            Ok(p) => p,
            Err(e) => return SqlResult::Error { message: e },
        };

        // Check for VECTOR_SEARCH in the WHERE clause
        if let Some(ref sel) = select.selection {
            if let Some(vs) = extract_vector_search(sel) {
                return self.execute_vector_search(&projection, &vs);
            }
        }

        let pending = self.pending_overlay();
        let mut rows = Vec::new();
        for (key, idx) in self.db.keymap.all_keys() {
            let visible = match selection_matches_key(select.selection.as_ref(), key) {
                Ok(v) => v,
                Err(e) => return SqlResult::Error { message: e },
            };
            if !visible {
                continue;
            }
            let cell = pending
                .get(&idx)
                .copied()
                .unwrap_or_else(|| self.db.state.values.get(idx).copied().unwrap_or(0));

            // Resolve typed value
            let tag = self.db.type_map.get(key);
            let blob = self.db.blob_store.get(key);
            let typed = match TypedValue::decode(tag, cell, blob) {
                Ok(v) => v,
                Err(e) => {
                    return SqlResult::Error {
                        message: format!("typed decode failed for key '{key}': {e}"),
                    };
                }
            };
            rows.push(render_projection_row(&projection, key, &typed, tag));
        }

        SqlResult::Rows {
            columns: projection.iter().map(|p| p.to_string()).collect(),
            rows,
        }
    }

    fn execute_vector_search(
        &self,
        projection: &[ProjectionField],
        vs: &VectorSearchParams,
    ) -> SqlResult {
        let results = match self
            .db
            .vector_index
            .search(&vs.query_vector, vs.k, vs.metric)
        {
            Ok(r) => r,
            Err(e) => return SqlResult::Error { message: e },
        };

        let mut cols: Vec<String> = projection.iter().map(|p| p.to_string()).collect();
        cols.push("_distance".to_string());

        let mut rows = Vec::new();
        for result in results {
            let key = &result.key;
            if let Some(typed) = self.db.get_typed(key) {
                let tag = self.db.type_map.get(key);
                let mut row = render_projection_row(projection, key, &typed, tag);
                row.push(format!("{:.6}", result.distance));
                rows.push(row);
            }
        }

        SqlResult::Rows {
            columns: cols,
            rows,
        }
    }

    fn execute_update(
        &mut self,
        table: &TableWithJoins,
        assignments: &[Assignment],
        selection: Option<&Expr>,
    ) -> SqlResult {
        if *self.db.write_mode() == WriteMode::AppendOnly {
            return SqlResult::Error {
                message:
                    "UPDATE rejected: database is in AppendOnly mode (immutable agentic records)"
                        .to_string(),
            };
        }
        if !table_with_joins_is_data(table) {
            return SqlResult::Error {
                message: format!("Only virtual table '{TABLE_NAME}' is supported"),
            };
        }
        let value = match extract_update_value(assignments) {
            Ok(v) => v,
            Err(e) => return SqlResult::Error { message: e },
        };

        let keys: Vec<(String, usize)> = self
            .db
            .keymap
            .all_keys()
            .map(|(k, idx)| (k.to_string(), idx))
            .collect();
        let mut touched = 0usize;
        for (key, idx) in keys {
            let visible = match selection_matches_key(selection, &key) {
                Ok(v) => v,
                Err(e) => return SqlResult::Error { message: e },
            };
            if visible {
                let (typed_idx, typed_cell) =
                    match self.db.put_typed(&key, TypedValue::Integer(value as i64)) {
                        Ok(v) => v,
                        Err(e) => {
                            return SqlResult::Error {
                                message: format!("typed update failed for '{key}': {e}"),
                            };
                        }
                    };
                debug_assert_eq!(typed_idx, idx);
                self.pending_writes.push((typed_idx, typed_cell));
                touched += 1;
            }
        }
        SqlResult::Ok {
            message: format!("Queued {touched} update(s)"),
        }
    }

    fn execute_delete(&mut self, del: &Delete) -> SqlResult {
        if *self.db.write_mode() == WriteMode::AppendOnly {
            return SqlResult::Error {
                message:
                    "DELETE rejected: database is in AppendOnly mode (immutable agentic records)"
                        .to_string(),
            };
        }
        if !delete_targets_data(del) {
            return SqlResult::Error {
                message: format!("Only virtual table '{TABLE_NAME}' is supported"),
            };
        }
        let keys: Vec<(String, usize)> = self
            .db
            .keymap
            .all_keys()
            .map(|(k, idx)| (k.to_string(), idx))
            .collect();
        let mut touched = 0usize;
        for (key, idx) in keys {
            let visible = match selection_matches_key(del.selection.as_ref(), &key) {
                Ok(v) => v,
                Err(e) => return SqlResult::Error { message: e },
            };
            if visible {
                let (typed_idx, typed_cell) = match self.db.put_typed(&key, TypedValue::Integer(0))
                {
                    Ok(v) => v,
                    Err(e) => {
                        return SqlResult::Error {
                            message: format!("typed delete failed for '{key}': {e}"),
                        };
                    }
                };
                debug_assert_eq!(typed_idx, idx);
                self.pending_writes.push((typed_idx, typed_cell));
                touched += 1;
            }
        }
        SqlResult::Ok {
            message: format!("Queued {touched} delete tombstone(s)"),
        }
    }

    fn pending_overlay(&self) -> BTreeMap<usize, u64> {
        let mut overlay = BTreeMap::new();
        for (idx, value) in &self.pending_writes {
            overlay.insert(*idx, *value);
        }
        overlay
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectionField {
    Key,
    Value,
    Type,
}

impl std::fmt::Display for ProjectionField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Key => f.write_str(COL_KEY),
            Self::Value => f.write_str(COL_VALUE),
            Self::Type => f.write_str(COL_TYPE),
        }
    }
}

fn normalize_command(sql: &str) -> String {
    sql.trim().trim_end_matches(';').trim().to_ascii_uppercase()
}

pub(crate) fn split_sql_statements(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_single_quote = false;
    let chars: Vec<char> = sql.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\'' {
            cur.push(ch);
            // SQL escapes single quote inside string as ''.
            if in_single_quote && i + 1 < chars.len() && chars[i + 1] == '\'' {
                cur.push(chars[i + 1]);
                i += 1;
            } else {
                in_single_quote = !in_single_quote;
            }
        } else if ch == ';' && !in_single_quote {
            let stmt = cur.trim();
            if !stmt.is_empty() {
                out.push(stmt.to_string());
            }
            cur.clear();
        } else {
            cur.push(ch);
        }
        i += 1;
    }
    let tail = cur.trim();
    if !tail.is_empty() {
        out.push(tail.to_string());
    }
    out
}

fn format_unix_utc(ts: u64) -> String {
    Utc.timestamp_opt(ts as i64, 0)
        .single()
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| format!("invalid_unix_ts({ts})"))
}

fn extract_single_quoted_argument(sql: &str) -> Option<String> {
    let start = sql.find('\'')?;
    let rem = &sql[start + 1..];
    let end_rel = rem.find('\'')?;
    Some(rem[..end_rel].to_string())
}

fn object_name_is_data(name: &ObjectName) -> bool {
    name.0
        .last()
        .and_then(ObjectNamePart::as_ident)
        .map(|id| id.value.eq_ignore_ascii_case(TABLE_NAME))
        .unwrap_or(false)
}

fn table_factor_is_data(f: &TableFactor) -> bool {
    match f {
        TableFactor::Table { name, .. } => object_name_is_data(name),
        _ => false,
    }
}

fn table_with_joins_is_data(t: &TableWithJoins) -> bool {
    t.joins.is_empty() && table_factor_is_data(&t.relation)
}

fn insert_targets_data(ins: &Insert) -> bool {
    match &ins.table {
        TableObject::TableName(name) => object_name_is_data(name),
        TableObject::TableFunction(_) => false,
    }
}

fn delete_targets_data(del: &Delete) -> bool {
    let tables = match &del.from {
        sqlparser::ast::FromTable::WithFromKeyword(v) => v,
        sqlparser::ast::FromTable::WithoutKeyword(v) => v,
    };
    tables.len() == 1 && table_with_joins_is_data(&tables[0])
}

fn ident_is(ident: &Ident, expected: &str) -> bool {
    ident.value.eq_ignore_ascii_case(expected)
}

fn resolve_projection(items: &[SelectItem]) -> Result<Vec<ProjectionField>, String> {
    if items.is_empty() {
        return Err("SELECT projection cannot be empty".to_string());
    }
    if items.len() == 1 {
        match &items[0] {
            SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(_, _) => {
                return Ok(vec![
                    ProjectionField::Key,
                    ProjectionField::Value,
                    ProjectionField::Type,
                ]);
            }
            _ => {}
        }
    }

    let mut out = Vec::new();
    for item in items {
        match item {
            SelectItem::UnnamedExpr(Expr::Identifier(id)) if ident_is(id, COL_KEY) => {
                out.push(ProjectionField::Key)
            }
            SelectItem::UnnamedExpr(Expr::Identifier(id)) if ident_is(id, COL_VALUE) => {
                out.push(ProjectionField::Value)
            }
            SelectItem::UnnamedExpr(Expr::Identifier(id)) if ident_is(id, COL_TYPE) => {
                out.push(ProjectionField::Type)
            }
            SelectItem::ExprWithAlias {
                expr: Expr::Identifier(id),
                ..
            } if ident_is(id, COL_KEY) => out.push(ProjectionField::Key),
            SelectItem::ExprWithAlias {
                expr: Expr::Identifier(id),
                ..
            } if ident_is(id, COL_VALUE) => out.push(ProjectionField::Value),
            SelectItem::ExprWithAlias {
                expr: Expr::Identifier(id),
                ..
            } if ident_is(id, COL_TYPE) => out.push(ProjectionField::Type),
            _ => {
                return Err(
                    "Only SELECT key, value, type (or SELECT *) projections are supported"
                        .to_string(),
                );
            }
        }
    }
    Ok(out)
}

fn render_projection_row(
    fields: &[ProjectionField],
    key: &str,
    typed: &TypedValue,
    tag: TypeTag,
) -> Vec<String> {
    fields
        .iter()
        .map(|f| match f {
            ProjectionField::Key => key.to_string(),
            ProjectionField::Value => typed.display_string(),
            ProjectionField::Type => tag.as_str().to_string(),
        })
        .collect()
}

/// Result of parsing a typed INSERT.
enum TypedInsert {
    Typed(String, TypedValue),
    LegacyU64(String, u64),
}

/// Extract a typed key-value from an INSERT statement.
fn extract_insert_typed(source: &Query, columns: &[Ident]) -> Result<TypedInsert, String> {
    let SetExpr::Values(values) = source.body.as_ref() else {
        return Err("INSERT only supports VALUES rows".to_string());
    };
    if values.rows.len() != 1 {
        return Err("INSERT requires exactly one VALUES row".to_string());
    }
    let row = &values.rows[0];
    let column_names: Vec<String> = if columns.is_empty() {
        vec![COL_KEY.to_string(), COL_VALUE.to_string()]
    } else {
        columns.iter().map(|c| c.value.clone()).collect()
    };
    if row.len() != column_names.len() {
        return Err("INSERT VALUES arity does not match column list".to_string());
    }

    let mut key: Option<String> = None;
    let mut value_expr: Option<&Expr> = None;

    for (col, expr) in column_names.iter().zip(row.iter()) {
        if col.eq_ignore_ascii_case(COL_KEY) {
            key = Some(expr_as_string(expr)?);
        } else if col.eq_ignore_ascii_case(COL_VALUE) {
            value_expr = Some(expr);
        } else {
            return Err(format!(
                "Unknown INSERT column '{col}'. Supported columns: {COL_KEY}, {COL_VALUE}"
            ));
        }
    }

    let key = key.ok_or_else(|| format!("Missing '{COL_KEY}' column"))?;
    let expr = value_expr.ok_or_else(|| format!("Missing '{COL_VALUE}' column"))?;

    // Try VECTOR() function
    if let Some(dims) = try_extract_vector(expr) {
        return Ok(TypedInsert::Typed(key, TypedValue::Vector(dims)));
    }

    // Try NULL
    if matches!(expr, Expr::Value(v) if matches!(&v.value, Value::Null)) {
        return Ok(TypedInsert::Typed(key, TypedValue::Null));
    }

    // Try boolean
    if matches!(expr, Expr::Value(v) if matches!(&v.value, Value::Boolean(true))) {
        return Ok(TypedInsert::Typed(key, TypedValue::Bool(true)));
    }
    if matches!(expr, Expr::Value(v) if matches!(&v.value, Value::Boolean(false))) {
        return Ok(TypedInsert::Typed(key, TypedValue::Bool(false)));
    }

    // Try number (integer or float)
    if let Expr::Value(v) = expr {
        if let Value::Number(n, _) = &v.value {
            // If it parses as u64, keep legacy behavior for backward compat
            if let Ok(u) = n.parse::<u64>() {
                return Ok(TypedInsert::LegacyU64(key, u));
            }
            // Try i64
            if let Ok(i) = n.parse::<i64>() {
                return Ok(TypedInsert::Typed(key, TypedValue::Integer(i)));
            }
            // Try f64
            if let Ok(f) = n.parse::<f64>() {
                return Ok(TypedInsert::Typed(key, TypedValue::Float(f)));
            }
        }
    }

    // Try string → auto-detect type
    if let Ok(s) = expr_as_string(expr) {
        let typed = infer_from_string(&s);
        return Ok(TypedInsert::Typed(key, typed));
    }

    Err("Cannot parse value expression".to_string())
}

/// Try to extract VECTOR(n1, n2, ...) or VECTOR([n1, n2, ...]) from an expression.
fn try_extract_vector(expr: &Expr) -> Option<Vec<f64>> {
    if let Expr::Function(func) = expr {
        let name = func.name.to_string().to_ascii_uppercase();
        if name == "VECTOR" || name == "VEC" || name == "EMBEDDING" {
            let args = match &func.args {
                sqlparser::ast::FunctionArguments::List(arg_list) => &arg_list.args,
                _ => return None,
            };
            let mut dims = Vec::new();
            for arg in args {
                match arg {
                    FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => {
                        if let Ok(f) = expr_as_f64(e) {
                            dims.push(f);
                        } else {
                            return None;
                        }
                    }
                    _ => return None,
                }
            }
            if !dims.is_empty() {
                return Some(dims);
            }
        }
    }
    None
}

/// Parse an expression as f64.
fn expr_as_f64(expr: &Expr) -> Result<f64, String> {
    match expr {
        Expr::Value(v) => match &v.value {
            Value::Number(n, _) => n
                .parse::<f64>()
                .map_err(|_| format!("Invalid float literal '{n}'")),
            _ => Err("Expected numeric literal".to_string()),
        },
        Expr::UnaryOp {
            op: sqlparser::ast::UnaryOperator::Minus,
            expr: inner,
        } => expr_as_f64(inner).map(|v| -v),
        _ => Err("Expected numeric literal".to_string()),
    }
}

/// Parameters extracted from a VECTOR_SEARCH() call in WHERE clause.
struct VectorSearchParams {
    query_vector: Vec<f64>,
    k: usize,
    metric: DistanceMetric,
}

/// Try to extract VECTOR_SEARCH(value, VECTOR(...), k [, 'metric']) from WHERE clause.
fn extract_vector_search(expr: &Expr) -> Option<VectorSearchParams> {
    if let Expr::Function(func) = expr {
        let name = func.name.to_string().to_ascii_uppercase();
        if name == "VECTOR_SEARCH" || name == "VSEARCH" || name == "KNN" {
            let args = match &func.args {
                sqlparser::ast::FunctionArguments::List(arg_list) => &arg_list.args,
                _ => return None,
            };
            if args.len() < 2 {
                return None;
            }

            // arg[0] = column ref (ignored, always 'value')
            // arg[1] = VECTOR(...) or list of numbers
            let query_vector = match &args[1] {
                FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => try_extract_vector(e)?,
                _ => return None,
            };

            // arg[2] = k (optional, default 10)
            let k = if args.len() > 2 {
                match &args[2] {
                    FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => expr_as_u64(e).ok()? as usize,
                    _ => 10,
                }
            } else {
                10
            };

            // arg[3] = metric (optional, default cosine)
            let metric = if args.len() > 3 {
                match &args[3] {
                    FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => {
                        let s = expr_as_string(e).ok()?;
                        DistanceMetric::from_str_tag(&s).unwrap_or(DistanceMetric::Cosine)
                    }
                    _ => DistanceMetric::Cosine,
                }
            } else {
                DistanceMetric::Cosine
            };

            return Some(VectorSearchParams {
                query_vector,
                k,
                metric,
            });
        }
    }
    None
}

fn extract_update_value(assignments: &[Assignment]) -> Result<u64, String> {
    if assignments.len() != 1 {
        return Err("UPDATE requires exactly one assignment: value = <u64>".to_string());
    }
    let assignment = &assignments[0];
    let target_ok = match &assignment.target {
        AssignmentTarget::ColumnName(name) => name
            .0
            .last()
            .and_then(ObjectNamePart::as_ident)
            .map(|id| id.value.eq_ignore_ascii_case(COL_VALUE))
            .unwrap_or(false),
        AssignmentTarget::Tuple(_) => false,
    };
    if !target_ok {
        return Err(format!("Only '{COL_VALUE}' can be assigned in UPDATE"));
    }
    expr_as_u64(&assignment.value)
}

fn selection_matches_key(selection: Option<&Expr>, key: &str) -> Result<bool, String> {
    let Some(expr) = selection else {
        return Ok(true);
    };
    eval_key_predicate(expr, key)
}

fn eval_key_predicate(expr: &Expr, key: &str) -> Result<bool, String> {
    match expr {
        Expr::Nested(inner) => eval_key_predicate(inner, key),
        Expr::BinaryOp { left, op, right } => match op {
            BinaryOperator::Eq => {
                if is_key_expr(left) {
                    Ok(key == expr_as_string(right)?)
                } else if is_key_expr(right) {
                    Ok(key == expr_as_string(left)?)
                } else {
                    Err("Only predicates on 'key' are supported".to_string())
                }
            }
            BinaryOperator::And => {
                Ok(eval_key_predicate(left, key)? && eval_key_predicate(right, key)?)
            }
            BinaryOperator::Or => {
                Ok(eval_key_predicate(left, key)? || eval_key_predicate(right, key)?)
            }
            _ => Err("Unsupported WHERE binary operator".to_string()),
        },
        Expr::Like {
            negated,
            any,
            expr,
            pattern,
            ..
        } => {
            if *any {
                return Err("LIKE ANY is not supported".to_string());
            }
            if !is_key_expr(expr) {
                return Err("LIKE is only supported on 'key'".to_string());
            }
            let pat = expr_as_string(pattern)?;
            let matched = like_match(key, &pat);
            Ok(if *negated { !matched } else { matched })
        }
        Expr::ILike {
            negated,
            any,
            expr,
            pattern,
            ..
        } => {
            if *any {
                return Err("ILIKE ANY is not supported".to_string());
            }
            if !is_key_expr(expr) {
                return Err("ILIKE is only supported on 'key'".to_string());
            }
            let pat = expr_as_string(pattern)?.to_ascii_lowercase();
            let matched = like_match(&key.to_ascii_lowercase(), &pat);
            Ok(if *negated { !matched } else { matched })
        }
        _ => Err("Unsupported WHERE predicate".to_string()),
    }
}

fn is_key_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Identifier(id) => ident_is(id, COL_KEY),
        Expr::CompoundIdentifier(parts) => {
            parts.last().map(|p| ident_is(p, COL_KEY)).unwrap_or(false)
        }
        _ => false,
    }
}

fn like_match(value: &str, pattern: &str) -> bool {
    if pattern == "%" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('%') {
        value.starts_with(prefix)
    } else {
        value == pattern
    }
}

fn expr_as_string(expr: &Expr) -> Result<String, String> {
    match expr {
        Expr::Value(v) => v
            .value
            .clone()
            .into_string()
            .ok_or_else(|| "Expected string literal".to_string()),
        Expr::Identifier(id) => Ok(id.value.clone()),
        _ => Err("Expected string literal".to_string()),
    }
}

fn expr_as_u64(expr: &Expr) -> Result<u64, String> {
    match expr {
        Expr::Value(v) => match &v.value {
            Value::Number(n, _) => n
                .parse::<u64>()
                .map_err(|_| format!("Invalid INTEGER literal '{n}'")),
            other => other
                .clone()
                .into_string()
                .ok_or_else(|| "Expected INTEGER literal".to_string())?
                .parse::<u64>()
                .map_err(|_| "Expected INTEGER literal".to_string()),
        },
        _ => Err("Expected INTEGER literal".to_string()),
    }
}
