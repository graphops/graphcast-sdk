use std::collections::HashMap;
use std::error::Error;
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
pub struct SlackBot {
    token: SlackApiToken,
}

impl SlackBot {
    pub async fn send_webhook(
        token_key: String,
        channel: &str,
        radio_name: &str,
        content: &str,
    ) -> Result<SlackApiChatPostMessageResponse, Box<dyn Error>> {
        let client = SlackClient::new(SlackClientHyperConnector::new());
        let token: SlackApiToken = SlackApiToken::new(token_key.into());
        let session = client.open_session(&token);
        let message =
            AlertMessageTemplateParams::create(radio_name.to_string(), content.to_string());

        let post_chat_req = SlackApiChatPostMessageRequest::new(
            SlackChannelId(channel.to_string()),
            message.render_template(),
        );
        let post_chat_resp = session.chat_post_message(&post_chat_req).await?;
        Ok(post_chat_resp)
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
