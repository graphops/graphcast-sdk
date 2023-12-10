use std::collections::HashMap;
use teloxide::types::ParseMode;
use thiserror::Error;

use rsb_derive::Builder;
use slack_morphism::prelude::*;

use teloxide::prelude::*;
use teloxide::types::ChatId;

// DiscordBot
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordBot {}

#[derive(Error, Debug)]
pub enum DiscordBotError {
    #[error("Request error: {0}")]
    RequestError(#[from] reqwest::Error),
}

impl DiscordBot {
    pub async fn send_webhook(
        webhook_url: &str,
        radio_name: &str,
        content: &str,
    ) -> Result<(), DiscordBotError> {
        let mut map = HashMap::new();
        map.insert(
            "content",
            format!("🚨 Notification from Radio '{radio_name}' \n{content}"),
        );

        let client = reqwest::Client::new();

        client.post(webhook_url).json(&map).send().await?;

        Ok(())
    }
}

// SlackBot
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackBot {}

#[derive(Error, Debug)]
pub enum SlackBotError {
    #[error("Request error: {0}")]
    RequestError(#[from] reqwest::Error),
}

impl SlackBot {
    pub async fn send_webhook(
        webhook_url: &str,
        radio_name: &str,
        content: &str,
    ) -> Result<(), SlackBotError> {
        let mut map = HashMap::new();
        map.insert(
            "text",
            format!("🚨 Notification from Radio '{radio_name}' \n{content}"),
        );

        let client = reqwest::Client::new();

        client.post(webhook_url).json(&map).send().await?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct TelegramBot {
    bot: Bot,
}

#[derive(Error, Debug)]
pub enum TelegramBotError {
    #[error("Request error: {0}")]
    RequestError(#[from] teloxide::RequestError),
}

impl TelegramBot {
    pub fn new(bot_token: String) -> Self {
        Self {
            bot: Bot::new(bot_token),
        }
    }

    pub async fn send_message(
        &self,
        chat_id: i64,
        radio_name: &str,
        content: &str,
    ) -> Result<(), TelegramBotError> {
        let message = format!(
            "🚨 Notification from Radio '{radio_name}' \n{content}",
            radio_name = radio_name,
            content = content
        );
        self.bot
            .send_message(ChatId(chat_id), message)
            .parse_mode(ParseMode::Html) // or ParseMode::MarkdownV2
            .disable_web_page_preview(true)
            .send()
            .await?;
        Ok(())
    }
}

#[derive(Debug, Clone, Builder)]
pub struct AlertMessageTemplateParams {
    pub user_id: Option<SlackUserId>,
    pub radio_name: String,
    pub content: String,
}

impl AlertMessageTemplateParams {
    pub fn create(radio_name: String, content: String) -> Self {
        Self {
            user_id: None,
            radio_name,
            content,
        }
    }
}

impl SlackMessageTemplate for AlertMessageTemplateParams {
    fn render_template(&self) -> SlackMessageContent {
        let user = match &self.user_id {
            Some(id) => format!(
                "🚨 Hello {}!, Notification from Radio '{}'",
                id.to_slack_format(),
                &self.radio_name
            ),
            None => format!("🚨 Notification from Radio '{}'", &self.radio_name),
        };
        SlackMessageContent::new().with_blocks(slack_blocks![
            some_into(SlackSectionBlock::new().with_text(pt!(user))),
            some_into(SlackDividerBlock::new()),
            some_into(SlackSectionBlock::new().with_text(pt!(&self.content)))
        ])
    }
}
