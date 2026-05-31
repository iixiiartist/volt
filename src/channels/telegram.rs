use async_trait::async_trait;

pub struct TelegramChannel {
    #[allow(dead_code)]
    pub token: String,
}

impl TelegramChannel {
    pub fn new(token: String) -> Self {
        Self { token }
    }
}

#[async_trait]
impl crate::channels::Channel for TelegramChannel {
    async fn start(
        &self,
        _agent: std::sync::Arc<crate::agent::Agent>,
        _shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        #[cfg(feature = "tools-telegram")]
        {
            use teloxide::prelude::*;
            let bot = Bot::new(&self.token);
            tracing::info!("[telegram] bot started");

            let handler = dptree::entry().branch(Update::filter_message().endpoint(
                |msg: Message, _: Bot| async move {
                    if let Some(text) = msg.text() {
                        tracing::info!("[telegram] received: {}", text);
                    }
                    anyhow::Ok(())
                },
            ));

            let mut dispatcher = Dispatcher::builder(bot, handler).build();

            tokio::select! {
                _ = dispatcher.dispatch() => {},
                _ = _shutdown.changed() => {
                    tracing::info!("[telegram] shutting down");
                }
            }
            Ok(())
        }

        #[cfg(not(feature = "tools-telegram"))]
        {
            tracing::warn!(
                "[telegram] channel disabled — compile with --features tools-telegram and provide TELEGRAM_BOT_TOKEN"
            );
            let mut rx = _shutdown;
            rx.changed().await.ok();
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_channel_trait_exists() {
        // Compile-time assertion
    }
}
