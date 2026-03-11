pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, crate::discord::handler::BotData, Error>;

fn io_error(msg: String) -> std::io::Error {
    std::io::Error::other(msg)
}

#[poise::command(slash_command)]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    let status = crate::discord::status::load_status().unwrap_or_default();
    ctx.say(format!(
        "connected: {} | guilds: {} | channels: {} | pending: {}",
        status.connected,
        status.guilds,
        status.channels.len(),
        status.pending_messages
    ))
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn verify(ctx: Context<'_>, channel_id: String, message_id: String) -> Result<(), Error> {
    let result = ctx
        .data()
        .recorder
        .verify_message(&channel_id, &message_id)
        .map_err(io_error)?;
    match result {
        Some((verified, value)) => {
            ctx.say(format!(
                "msg:{channel_id}:{message_id} | verified={verified} | value={value}"
            ))
            .await?;
        }
        None => {
            ctx.say("message not found").await?;
        }
    }
    Ok(())
}

#[poise::command(slash_command)]
pub async fn search(ctx: Context<'_>, query: String) -> Result<(), Error> {
    let rows = ctx
        .data()
        .recorder
        .search(&query, None, 10)
        .map_err(io_error)?;
    if rows.is_empty() {
        ctx.say("no matches").await?;
        return Ok(());
    }
    let lines = rows
        .into_iter()
        .map(|record| {
            format!(
                "#{} {}: {}",
                record.channel_name,
                record.author_name,
                trim(&record.content, 80)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    ctx.say(lines).await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn history(
    ctx: Context<'_>,
    channel_id: Option<String>,
    limit: Option<usize>,
) -> Result<(), Error> {
    let rows = ctx
        .data()
        .recorder
        .recent(channel_id.as_deref(), limit.unwrap_or(10).min(25))
        .map_err(io_error)?;
    if rows.is_empty() {
        ctx.say("no recorded messages").await?;
        return Ok(());
    }
    let body = rows
        .into_iter()
        .map(|record| {
            format!(
                "{} {} {} [{}]",
                record.message_id,
                record.channel_name,
                record.author_name,
                &record.record_seal[..12.min(record.record_seal.len())]
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    ctx.say(body).await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn export(ctx: Context<'_>, channel_id: String) -> Result<(), Error> {
    let rows = ctx
        .data()
        .recorder
        .export_channel(&channel_id)
        .map_err(io_error)?;
    let dir = crate::config::discord_export_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("discord_export_{channel_id}.json"));
    std::fs::write(&path, serde_json::to_vec_pretty(&rows)?)?;
    ctx.say(format!(
        "exported {} records to {}",
        rows.len(),
        path.display()
    ))
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn channels(ctx: Context<'_>) -> Result<(), Error> {
    let channels = ctx.data().recorder.recorded_channels().map_err(io_error)?;
    if channels.is_empty() {
        ctx.say("no recorded channels yet").await?;
        return Ok(());
    }
    let body = channels
        .into_iter()
        .map(|channel| {
            format!(
                "{} ({}) records={} last={}",
                channel.channel_name,
                channel.channel_id,
                channel.records,
                channel.last_message_id.unwrap_or_else(|| "-".to_string())
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    ctx.say(body).await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn integrity(ctx: Context<'_>) -> Result<(), Error> {
    let (append_only, seals) = ctx.data().recorder.integrity_summary().map_err(io_error)?;
    ctx.say(format!("append_only={append_only}, seals={seals}"))
        .await?;
    Ok(())
}

fn trim(input: &str, max_chars: usize) -> String {
    let mut out = input.chars().take(max_chars).collect::<String>();
    if input.chars().count() > max_chars {
        out.push('…');
    }
    out
}
