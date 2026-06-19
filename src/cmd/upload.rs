use anyhow::Result;
use tokio::runtime::Runtime;

use crate::api::{self, RecordingChangeset};
use crate::asciicast;
use crate::cli;
use crate::config::Config;

impl cli::Upload {
    pub fn run(self) -> Result<()> {
        Runtime::new()?.block_on(self.do_run())
    }

    async fn do_run(self) -> Result<()> {
        let mut config = Config::new(self.server_url.clone())?;
        let _ = asciicast::open_from_path(&self.file)?;

        let visibility = self.visibility.map(|v| match v {
            cli::Visibility::Public => api::Visibility::Public,
            cli::Visibility::Unlisted => api::Visibility::Unlisted,
            cli::Visibility::Private => api::Visibility::Private,
        });

        let changeset = RecordingChangeset {
            title: self.title.map(Some),
            description: self.description.map(Some),
            visibility,
            audio_url: self.audio_url.map(Some),
        };

        let response = api::create_recording(&self.file, changeset, &mut config).await?;
        println!("{}", response.message.unwrap_or(response.url));

        Ok(())
    }
}
