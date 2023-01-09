use rsb_derive::Builder;
use slack_morphism::prelude::*;
use std::error::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackBot {
    token: SlackApiToken,
}

impl SlackBot {
    /// Create a session to send content of a radio topic in slack channel
    pub async fn send_webhook(
        token_key: String,
        channel: &String,
        radio_name: String,
        content: String,
    ) -> Result<SlackApiChatPostMessageResponse, Box<dyn Error>> {
        let client = SlackClient::new(SlackClientHyperConnector::new());
        let token: SlackApiToken = SlackApiToken::new(token_key.into());
        let session = client.open_session(&token);
        let message = AlertMessageTemplateParams::new(radio_name, content);

        let post_chat_req =
            SlackApiChatPostMessageRequest::new(channel.into(), message.render_template());
        let post_chat_resp = session.chat_post_message(&post_chat_req).await?;
        Ok(post_chat_resp)
    }
}

#[derive(Debug, Clone, Builder)]
pub struct AlertMessageTemplateParams {
    pub user_id: Option<SlackUserId>,
    pub radio_name: String,
    pub content: String,
}

/// Render slack message to send
impl SlackMessageTemplate for AlertMessageTemplateParams {
    fn render_template(&self) -> SlackMessageContent {
        let user = match &self.user_id {
            Some(id) => format!(
                "Hey {}, on the topic of {}",
                id.to_slack_format(),
                &self.radio_name
            ),
            None => format!("Hi there! on the topic of {}", &self.radio_name),
        };
        SlackMessageContent::new().with_blocks(slack_blocks![
            // some_into(SlackHeaderBlock::new(pt!(user))),
            some_into(SlackSectionBlock::new().with_text(pt!(user))),
            some_into(SlackDividerBlock::new()),
            some_into(SlackSectionBlock::new().with_text(pt!(&self.content)))
        ])
    }
}
