use crate::discord::config::DiscordConfig;
use crate::discord::recorder::{from_message, DiscordRecorder};
use poise::serenity_prelude as serenity;
use serenity::{async_trait, model::prelude::*};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct BotData {
    pub config: DiscordConfig,
    pub recorder: DiscordRecorder,
}

#[derive(Clone)]
pub enum RecordEvent {
    Message(crate::discord::schema::DiscordMessageRecord),
    Edit(crate::discord::schema::DiscordMessageRecord),
    Delete {
        channel_id: ChannelId,
        message_id: MessageId,
    },
}

pub async fn run(config: DiscordConfig) -> Result<(), String> {
    if config.dry_run {
        return Ok(());
    }
    let recorder = DiscordRecorder::new(config.db_path.clone());
    recorder.ensure_db()?;
    let (tx, mut rx) = mpsc::channel::<RecordEvent>(1024);
    let shared_cfg = config.clone();
    let batch_recorder = recorder.clone();
    tokio::spawn(async move {
        let mut message_batch = Vec::new();
        let mut edit_batch = Vec::new();
        let mut delete_batch = Vec::new();
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            shared_cfg.batch_timeout_secs,
        ));
        loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    match event {
                        RecordEvent::Message(record) => message_batch.push(record),
                        RecordEvent::Edit(record) => edit_batch.push(record),
                        RecordEvent::Delete { channel_id, message_id } => delete_batch.push((channel_id, message_id)),
                    }
                    let pending = message_batch.len() + edit_batch.len() + delete_batch.len();
                    let _ = batch_recorder.update_status(|status| {
                        status.pending_messages = pending;
                    });
                    if pending >= shared_cfg.batch_size {
                        flush_batch(&batch_recorder, &mut message_batch, &mut edit_batch, &mut delete_batch);
                    }
                }
                _ = interval.tick() => {
                    if !message_batch.is_empty() || !edit_batch.is_empty() || !delete_batch.is_empty() {
                        flush_batch(&batch_recorder, &mut message_batch, &mut edit_batch, &mut delete_batch);
                    }
                }
            }
        }
    });

    let setup_tx = tx.clone();
    let setup_config = config.clone();
    let client_config = config.clone();
    let recorder_for_setup = recorder.clone();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                crate::discord::commands::status(),
                crate::discord::commands::verify(),
                crate::discord::commands::search(),
                crate::discord::commands::history(),
                crate::discord::commands::export(),
                crate::discord::commands::channels(),
                crate::discord::commands::integrity(),
            ],
            ..Default::default()
        })
        .setup(move |ctx, ready, framework| {
            let _tx = setup_tx.clone();
            let config = setup_config.clone();
            let recorder = recorder_for_setup.clone();
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands)
                    .await
                    .map_err(|e| format!("register slash commands: {e}"))?;
                backfill_missing_messages(ctx, ready, &config, &recorder).await?;
                recorder.rebuild_status(true, ready.guilds.len())?;
                Ok(BotData { config, recorder })
            })
        })
        .build();

    let intents = serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::MESSAGE_CONTENT
        | serenity::GatewayIntents::GUILDS;
    let mut client = serenity::ClientBuilder::new(&client_config.token, intents)
        .framework(framework)
        .event_handler(Handler {
            tx,
            config: client_config.clone(),
            recorder: recorder.clone(),
        })
        .await
        .map_err(|e| format!("build discord client: {e}"))?;
    client
        .start()
        .await
        .map_err(|e| format!("discord client error: {e}"))
}

fn flush_batch(
    recorder: &DiscordRecorder,
    messages: &mut Vec<crate::discord::schema::DiscordMessageRecord>,
    edits: &mut Vec<crate::discord::schema::DiscordMessageRecord>,
    deletes: &mut Vec<(ChannelId, MessageId)>,
) {
    let flush_messages = std::mem::take(messages);
    let flush_edits = std::mem::take(edits);
    let flush_deletes = std::mem::take(deletes);
    let _ = recorder.record_messages(&flush_messages);
    let _ = recorder.record_edits(&flush_edits);
    let _ = recorder.record_deletes(&flush_deletes);
    let _ = recorder.update_status(|status| {
        status.pending_messages = 0;
        status.last_commit_at = Some(chrono::Utc::now().to_rfc3339());
    });
}

struct Handler {
    tx: mpsc::Sender<RecordEvent>,
    config: DiscordConfig,
    recorder: DiscordRecorder,
}

#[async_trait]
impl serenity::EventHandler for Handler {
    async fn message(&self, ctx: serenity::Context, msg: Message) {
        if msg.author.bot && !self.config.record_bots {
            return;
        }
        if !self.config.should_record_channel(msg.channel_id.get()) {
            return;
        }
        let channel_name = msg.channel_id.to_string();
        let guild_name = msg
            .guild_id
            .and_then(|id| ctx.cache.guild(id))
            .map(|g| g.name.to_string());
        let record = from_message(&msg, channel_name.clone(), guild_name);
        let _ = self.tx.send(RecordEvent::Message(record)).await;
        let _ = self.recorder.update_status(|status| {
            status.connected = true;
            status.pending_messages += 1;
        });
    }

    async fn message_update(
        &self,
        ctx: serenity::Context,
        _old_if_available: Option<Message>,
        _new_if_available: Option<Message>,
        event: MessageUpdateEvent,
    ) {
        if !self.config.record_edits {
            return;
        }
        if !self.config.should_record_channel(event.channel_id.get()) {
            return;
        }
        if let Ok(msg) = event.channel_id.message(&ctx.http, event.id).await {
            let channel_name = msg.channel_id.to_string();
            let guild_name = msg
                .guild_id
                .and_then(|id| ctx.cache.guild(id))
                .map(|g| g.name.to_string());
            let record = from_message(&msg, channel_name, guild_name);
            let _ = self.tx.send(RecordEvent::Edit(record)).await;
        }
    }

    async fn message_delete(
        &self,
        _ctx: serenity::Context,
        channel_id: ChannelId,
        deleted_message_id: MessageId,
        _guild_id: Option<GuildId>,
    ) {
        if !self.config.record_deletes {
            return;
        }
        if !self.config.should_record_channel(channel_id.get()) {
            return;
        }
        let _ = self
            .tx
            .send(RecordEvent::Delete {
                channel_id,
                message_id: deleted_message_id,
            })
            .await;
    }
}

async fn backfill_missing_messages(
    ctx: &serenity::Context,
    ready: &Ready,
    config: &DiscordConfig,
    recorder: &DiscordRecorder,
) -> Result<(), String> {
    let channel_ids = discover_channels(ctx, ready, config).await;
    for channel_id in channel_ids {
        backfill_channel(ctx, config, recorder, channel_id).await?;
    }
    Ok(())
}

async fn discover_channels(
    ctx: &serenity::Context,
    ready: &Ready,
    config: &DiscordConfig,
) -> Vec<ChannelId> {
    if let Some(ids) = &config.channels {
        return ids.iter().copied().map(ChannelId::new).collect();
    }
    let mut discovered = Vec::new();
    for guild in &ready.guilds {
        if let Ok(channels) = guild.id.channels(&ctx.http).await {
            discovered.extend(channels.into_keys());
        }
    }
    discovered.sort_by_key(|id| id.get());
    discovered.dedup_by_key(|id| id.get());
    discovered
}

async fn backfill_channel(
    ctx: &serenity::Context,
    config: &DiscordConfig,
    recorder: &DiscordRecorder,
    channel_id: ChannelId,
) -> Result<(), String> {
    if !config.should_record_channel(channel_id.get()) {
        return Ok(());
    }
    let mut after = recorder.last_recorded_message_id(channel_id)?;
    loop {
        let mut request = serenity::GetMessages::new().limit(100);
        if let Some(after_id) = after {
            request = request.after(after_id);
        }
        let mut messages = match channel_id.messages(&ctx.http, request).await {
            Ok(messages) => messages,
            Err(_) => return Ok(()),
        };
        if messages.is_empty() {
            break;
        }
        messages.sort_by_key(|message| message.id);
        after = messages.last().map(|message| message.id);
        let records = messages
            .into_iter()
            .filter(|message| config.record_bots || !message.author.bot)
            .map(|message| {
                let guild_name = message
                    .guild_id
                    .and_then(|id| ctx.cache.guild(id))
                    .map(|g| g.name.to_string());
                from_message(&message, message.channel_id.to_string(), guild_name)
            })
            .collect::<Vec<_>>();
        if !records.is_empty() {
            recorder.record_messages(&records)?;
        }
        if records.len() < 100 {
            break;
        }
    }
    Ok(())
}
