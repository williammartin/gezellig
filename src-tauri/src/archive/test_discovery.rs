use futures_util::StreamExt;
use librespot::core::config::SessionConfig;
use librespot::discovery::Discovery;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().json().init();

    tracing::info!(event = "discovery_start", message = "Starting Zeroconf discovery test (dns-sd backend)");

    let config = SessionConfig::default();
    tracing::info!(event = "discovery_config", device_id = %config.device_id, client_id = %config.client_id);

    match Discovery::builder(&config.device_id, &config.client_id)
        .name("Gezellig DJ")
        .launch()
    {
        Ok(mut discovery) => {
            tracing::info!(event = "discovery_launched");
            tracing::info!(event = "discovery_waiting", message = "Waiting 60s for credentials");

            let timeout = tokio::time::sleep(std::time::Duration::from_secs(60));
            tokio::pin!(timeout);

            loop {
                tokio::select! {
                    creds = discovery.next() => {
                        match creds {
                            Some(creds) => {
                                tracing::info!(event = "discovery_credentials", username = ?creds.username);
                                break;
                            }
                            None => {
                                tracing::info!(event = "discovery_stream_ended");
                                break;
                            }
                        }
                    }
                    _ = &mut timeout => {
                        tracing::warn!(event = "discovery_timeout", seconds = 60);
                        break;
                    }
                }
            }
            tracing::info!(event = "discovery_done");
        }
        Err(e) => {
            tracing::error!(event = "discovery_failed", error = ?e);
        }
    }
}
