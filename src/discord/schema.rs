use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscordAttachment {
    pub id: String,
    pub filename: String,
    pub size: u64,
    pub url: String,
    pub content_type: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscordMessageRecord {
    pub message_id: String,
    pub channel_id: String,
    pub channel_name: String,
    pub guild_id: Option<String>,
    pub guild_name: Option<String>,
    pub author_id: String,
    pub author_name: String,
    pub author_discriminator: Option<String>,
    pub author_bot: bool,
    pub content: String,
    pub timestamp: String,
    pub edited_timestamp: Option<String>,
    pub attachments: Vec<DiscordAttachment>,
    pub embeds_count: usize,
    pub mentions: Vec<String>,
    pub reference_message_id: Option<String>,
    pub thread_id: Option<String>,
    pub reactions: Vec<String>,
    pub recorded_at: String,
    pub record_seal: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct DiscordChannelStatus {
    pub channel_id: String,
    pub channel_name: String,
    pub records: usize,
    pub last_message_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct DiscordBotStatus {
    pub connected: bool,
    pub guilds: usize,
    pub channels: Vec<DiscordChannelStatus>,
    pub last_commit_at: Option<String>,
    pub pending_messages: usize,
}
