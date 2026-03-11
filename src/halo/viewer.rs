use crate::halo::trace::{
    cost_buckets, list_sessions, paid_breakdown_by_operation_type, paid_cost_buckets,
    session_events, session_summary,
};
use serde_json::json;
use std::path::Path;

pub fn print_traces(db_path: &Path, maybe_session_id: Option<&str>) -> Result<(), String> {
    if let Some(session_id) = maybe_session_id {
        let sessions = list_sessions(db_path)?;
        let meta = sessions
            .into_iter()
            .find(|s| s.session_id == session_id)
            .ok_or_else(|| format!("session not found: {session_id}"))?;
        println!("Session: {}", meta.session_id);
        println!("Agent: {}", meta.agent);
        println!(
            "Model: {}",
            meta.model.clone().unwrap_or_else(|| "unknown".to_string())
        );
        println!("Status: {:?}", meta.status);
        println!("Started: {}", format_timestamp(meta.started_at));
        println!(
            "Ended: {}",
            meta.ended_at
                .map(format_timestamp)
                .unwrap_or_else(|| "in progress".to_string())
        );
        if let Some(summary) = session_summary(db_path, session_id)? {
            println!(
                "Tokens in/out: {}/{}",
                summary.total_input_tokens, summary.total_output_tokens
            );
            println!("Cost: ${:.4}", summary.estimated_cost_usd);
            println!("Duration: {}s", summary.duration_secs);
        }
        println!();
        println!("Event timeline:");
        let events = session_events(db_path, session_id)?;
        for ev in events {
            println!(
                "  {:>5}  {:<16}  {}",
                ev.seq,
                format!("{:?}", ev.event_type),
                compact_json(&ev.content)
            );
        }
        return Ok(());
    }

    let sessions = list_sessions(db_path)?;
    let mut rows = Vec::new();
    for s in sessions {
        let summary = session_summary(db_path, &s.session_id)?;
        let token_total = summary
            .as_ref()
            .map(|v| v.total_input_tokens + v.total_output_tokens)
            .unwrap_or(0);
        let cost = summary
            .as_ref()
            .map(|v| v.estimated_cost_usd)
            .unwrap_or(0.0);
        let duration = summary.as_ref().map(|v| v.duration_secs).unwrap_or(0);
        rows.push(vec![
            s.session_id,
            s.agent,
            s.model.unwrap_or_else(|| "unknown".to_string()),
            token_total.to_string(),
            format!("${cost:.4}"),
            format_duration(duration),
            format!("{:?}", s.status).to_ascii_lowercase(),
        ]);
    }

    print_table(
        &[
            "Session ID",
            "Agent",
            "Model",
            "Tokens",
            "Cost",
            "Duration",
            "Status",
        ],
        &rows,
    );
    Ok(())
}

pub fn print_costs(db_path: &Path, monthly: bool) -> Result<(), String> {
    let rows = cost_buckets(db_path, monthly)?;
    if rows.is_empty() {
        println!("No recorded costs yet.");
        return Ok(());
    }

    let mut total_sessions = 0u64;
    let mut total_tokens = 0u64;
    let mut total_cost = 0.0f64;

    let table_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            total_sessions += r.sessions;
            total_tokens += r.input_tokens + r.output_tokens;
            total_cost += r.cost_usd;
            vec![
                r.label.clone(),
                r.sessions.to_string(),
                format_number(r.input_tokens + r.output_tokens),
                format!("${:.4}", r.cost_usd),
            ]
        })
        .collect();

    print_table(&["Bucket", "Sessions", "Tokens", "Cost"], &table_rows);
    println!(
        "TOTAL: sessions={} tokens={} cost=${:.4}",
        total_sessions,
        format_number(total_tokens),
        total_cost
    );
    Ok(())
}

pub fn print_paid_costs(db_path: &Path, monthly: bool) -> Result<(), String> {
    let buckets = paid_cost_buckets(db_path, monthly)?;
    if buckets.is_empty() {
        println!("No paid operations recorded yet.");
        return Ok(());
    }

    let mut total_ops = 0u64;
    let mut total_credits = 0u64;
    let mut total_usd = 0.0f64;
    let rows: Vec<Vec<String>> = buckets
        .iter()
        .map(|bucket| {
            total_ops += bucket.operations;
            total_credits = total_credits.saturating_add(bucket.credits_spent);
            total_usd += bucket.usd_spent;
            vec![
                bucket.label.clone(),
                bucket.operations.to_string(),
                format_number(bucket.credits_spent),
                format!("${:.2}", bucket.usd_spent),
            ]
        })
        .collect();
    print_table(
        &["Bucket", "Operations", "Credits Spent", "USD Spent"],
        &rows,
    );
    println!(
        "TOTAL: operations={} credits={} usd=${:.2}",
        total_ops,
        format_number(total_credits),
        total_usd
    );

    let by_type = paid_breakdown_by_operation_type(db_path)?;
    if !by_type.is_empty() {
        println!();
        println!("By operation type:");
        let type_rows: Vec<Vec<String>> = by_type
            .into_iter()
            .map(|(operation_type, count, credits, usd)| {
                vec![
                    operation_type,
                    count.to_string(),
                    format_number(credits),
                    format!("${:.2}", usd),
                ]
            })
            .collect();
        print_table(
            &["Operation", "Count", "Credits Spent", "USD Spent"],
            &type_rows,
        );
    }
    Ok(())
}

pub fn print_traces_json(db_path: &Path, maybe_session_id: Option<&str>) -> Result<(), String> {
    if let Some(session_id) = maybe_session_id {
        let sessions = list_sessions(db_path)?;
        let meta = sessions
            .into_iter()
            .find(|s| s.session_id == session_id)
            .ok_or_else(|| format!("session not found: {session_id}"))?;
        let summary = session_summary(db_path, session_id)?;
        let events = session_events(db_path, session_id)?;
        let out = json!({
            "session": meta,
            "summary": summary,
            "events": events,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&out)
                .map_err(|e| format!("serialize traces json: {e}"))?
        );
        return Ok(());
    }

    let sessions = list_sessions(db_path)?;
    let mut items = Vec::new();
    for s in sessions {
        let summary = session_summary(db_path, &s.session_id)?;
        items.push(json!({
            "session": s,
            "summary": summary,
        }));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&items).map_err(|e| format!("serialize traces json: {e}"))?
    );
    Ok(())
}

pub fn print_costs_json(db_path: &Path, monthly: bool) -> Result<(), String> {
    let rows = cost_buckets(db_path, monthly)?;
    let total_cost: f64 = rows.iter().map(|r| r.cost_usd).sum();
    let total_tokens: u64 = rows.iter().map(|r| r.input_tokens + r.output_tokens).sum();
    let total_sessions: u64 = rows.iter().map(|r| r.sessions).sum();

    let items: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            json!({
                "label": r.label,
                "sessions": r.sessions,
                "input_tokens": r.input_tokens,
                "output_tokens": r.output_tokens,
                "cache_tokens": r.cache_tokens,
                "cost_usd": r.cost_usd,
            })
        })
        .collect();

    let out = json!({
        "buckets": items,
        "total_sessions": total_sessions,
        "total_tokens": total_tokens,
        "total_cost_usd": total_cost,
        "granularity": if monthly { "monthly" } else { "daily" },
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&out).map_err(|e| format!("serialize costs json: {e}"))?
    );
    Ok(())
}

pub fn print_status(db_path: &Path, json_mode: bool) -> Result<(), String> {
    let sessions = list_sessions(db_path)?;
    let session_count = sessions.len();
    let latest = sessions.first().cloned();

    let mut total_cost = 0.0f64;
    let mut total_tokens = 0u64;
    for s in &sessions {
        if let Ok(Some(summary)) = session_summary(db_path, &s.session_id) {
            total_cost += summary.estimated_cost_usd;
            total_tokens += summary.total_input_tokens + summary.total_output_tokens;
        }
    }

    if json_mode {
        let out = json!({
            "session_count": session_count,
            "total_cost_usd": total_cost,
            "total_tokens": total_tokens,
            "latest_session": latest,
            "db_path": db_path.to_string_lossy(),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&out)
                .map_err(|e| format!("serialize status json: {e}"))?
        );
    } else {
        println!("AgentHALO Status");
        println!("  Sessions recorded: {session_count}");
        println!("  Total tokens:      {}", format_number(total_tokens));
        println!("  Total cost:        ${total_cost:.4}");
        println!("  Database:          {}", db_path.display());
        if let Some(latest) = latest {
            println!();
            println!("Latest session:");
            println!("  ID:    {}", latest.session_id);
            println!("  Agent: {}", latest.agent);
            println!(
                "  Model: {}",
                latest.model.unwrap_or_else(|| "unknown".to_string())
            );
            println!("  Time:  {}", format_timestamp(latest.started_at));
            println!("  Status: {:?}", latest.status);
        }
    }

    Ok(())
}

pub fn export_session_json(db_path: &Path, session_id: &str) -> Result<serde_json::Value, String> {
    let sessions = list_sessions(db_path)?;
    let meta = sessions
        .into_iter()
        .find(|s| s.session_id == session_id)
        .ok_or_else(|| format!("session not found: {session_id}"))?;
    let summary = session_summary(db_path, session_id)?;
    let events = session_events(db_path, session_id)?;
    Ok(json!({
        "version": "agenthalo-export-v1",
        "session": meta,
        "summary": summary,
        "events": events,
        "event_count": events.len(),
    }))
}

fn print_table(columns: &[&str], rows: &[Vec<String>]) {
    let mut widths: Vec<usize> = columns.iter().map(|c| c.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }

    let sep = widths
        .iter()
        .map(|w| "-".repeat(*w + 2))
        .collect::<Vec<_>>()
        .join("+");

    let header = columns
        .iter()
        .enumerate()
        .map(|(i, c)| format!(" {:width$} ", c, width = widths[i]))
        .collect::<Vec<_>>()
        .join("|");
    println!("{header}");
    println!("{sep}");

    for row in rows {
        let line = widths
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let cell = row.get(i).cloned().unwrap_or_default();
                format!(" {:width$} ", cell, width = *w)
            })
            .collect::<Vec<_>>()
            .join("|");
        println!("{line}");
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let keep = max.saturating_sub(3);
    format!("{}...", &s[..keep])
}

fn format_duration(mut secs: u64) -> String {
    let h = secs / 3600;
    secs %= 3600;
    let m = secs / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h {m}m {s}s")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

fn format_number(v: u64) -> String {
    let s = v.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn format_timestamp(ts: u64) -> String {
    chrono::DateTime::from_timestamp(ts as i64, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| ts.to_string())
}

fn compact_json(v: &serde_json::Value) -> String {
    let raw = serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string());
    truncate(&raw, 120)
}
