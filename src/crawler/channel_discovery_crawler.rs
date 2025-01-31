use crate::{
    commands::crawl_channel_command::CrawlChannelCommand,
    repos::{
        additional_channel_repo::AdditionalChannelRepository, channel_repo::ChannelRepository,
        settings_repo::SettingsRepository,
    },
    services::{guitar_terms_service::GuitarTermsService, youtube_service::YoutubeService},
    utils::consts::ONE_DAYS_IN_SECONDS,
};
use anyhow::Error;
use chrono::Utc;
use log::info;
use std::time::Duration;
use tokio::sync::mpsc::Sender;
use tokio::time::sleep;

pub struct ChannelDiscoveryCrawler {
    sender: Sender<CrawlChannelCommand>,
    channel_repo: ChannelRepository,
    settings_repo: SettingsRepository,
    youtube_service: YoutubeService,
    guitar_terms_service: GuitarTermsService,
    additional_channel_repo: AdditionalChannelRepository,
}

impl ChannelDiscoveryCrawler {
    pub fn new(
        sender: Sender<CrawlChannelCommand>,
        channel_repo: ChannelRepository,
        settings_repo: SettingsRepository,
        youtube_service: YoutubeService,
        guitar_terms_service: GuitarTermsService,
        additional_channel_repo: AdditionalChannelRepository,
    ) -> ChannelDiscoveryCrawler {
        ChannelDiscoveryCrawler {
            sender,
            channel_repo,
            settings_repo,
            youtube_service,
            guitar_terms_service,
            additional_channel_repo,
        }
    }

    pub async fn crawl(&self) -> Result<(), Error> {
        println!("Start channel discovery crawler");

        loop {
            if self.should_crawl().await.unwrap_or(false) {
                let channel_ids = self.channel_repo.get_ids_upload_last_month(8000).await?;

                for channel_id in channel_ids {
                    info!("Check subscriptions of channel {}", channel_id);

                    let subscriptions = self
                        .youtube_service
                        .get_channel_subscriptions(&channel_id)
                        .await
                        .unwrap_or(vec![]);

                    for snippet in subscriptions {
                        let sub_channel_id = snippet.resource_id.channel_id;

                        let guitar_terms_result = self
                            .guitar_terms_service
                            .has_guitar_term(
                                &sub_channel_id,
                                &snippet.title,
                                &snippet.description,
                                false,
                            )
                            .await;

                        let is_newly_discovered =
                            self.is_channel_newly_discovered(&sub_channel_id).await?;

                        let is_not_non_guitar_channel = self
                            .guitar_terms_service
                            .is_not_listed_as_non_guitar_channel(&sub_channel_id)
                            .await;

                        if is_newly_discovered
                            && is_not_non_guitar_channel
                            && guitar_terms_result.has_guitar_term
                        {
                            info!("Send channel for crawling: {}", sub_channel_id);

                            let cmd = CrawlChannelCommand {
                                channel_id: sub_channel_id.clone(),
                                ignore_guitar_terms: false,
                            };

                            self.sender.send(cmd).await?;
                        } else {
                            info!("Channel {} does not qualify as a newly discovered channel (is_newly_discovered = {}, is_not_non_guitar_channel = {}, has_guitar_term = {})", sub_channel_id, is_newly_discovered, is_not_non_guitar_channel, guitar_terms_result.has_guitar_term);
                        }
                    }
                }

                let crawl_timestamp = Utc::now().timestamp();
                self.settings_repo
                    .set_last_discovery_crawl(crawl_timestamp)
                    .await;
            }

            info!("Wait for {} seconds until next crawl", ONE_DAYS_IN_SECONDS);

            sleep(Duration::from_secs(ONE_DAYS_IN_SECONDS)).await;
        }
    }

    async fn should_crawl(&self) -> Result<bool, Error> {
        let last_crawl_timestamp = self.settings_repo.get_last_discovery_crawl().await?;
        let seconds_since_last_crawl = Utc::now().timestamp() - last_crawl_timestamp;

        Ok(seconds_since_last_crawl >= ONE_DAYS_IN_SECONDS as i64)
    }

    async fn is_channel_newly_discovered(&self, channel_id: &str) -> Result<bool, Error> {
        let channel_exists = self.channel_repo.exists(channel_id).await?;
        let additional_exists = self.additional_channel_repo.exists(channel_id).await?;

        Ok(!channel_exists && !additional_exists)
    }
}
