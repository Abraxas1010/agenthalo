use crate::halo::trace::{cost_buckets, list_sessions, session_events, session_summary};
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
        println!("Started: {}", meta.started_at);
        println!("Ended: {}", meta.ended_at.unwrap_or(0));
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

fn compact_json(v: &serde_json::Value) -> String {
    let raw = serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string());
    truncate(&raw, 120)
}
