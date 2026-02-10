use futures_util::StreamExt;
use librespot::core::config::SessionConfig;
use librespot::discovery::Discovery;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .init();

    eprintln!("Starting Zeroconf discovery test (dns-sd backend)...");

    let config = SessionConfig::default();
    eprintln!("device_id: {}", config.device_id);
    eprintln!("client_id: {}", config.client_id);

    match Discovery::builder(&config.device_id, &config.client_id)
        .name("Gezellig DJ")
        .launch()
    {
        Ok(mut discovery) => {
            eprintln!("✅ Discovery launched successfully!");
            eprintln!("Check Spotify for 'Gezellig DJ' device. Waiting 60s...");
            eprintln!("Will print if credentials are received...");

            let timeout = tokio::time::sleep(std::time::Duration::from_secs(60));
            tokio::pin!(timeout);

            loop {
                tokio::select! {
                    creds = discovery.next() => {
                        match creds {
                            Some(creds) => {
                                eprintln!("🎉 Got credentials! username: {:?}", creds.username);
                                break;
                            }
                            None => {
                                eprintln!("Discovery stream ended");
                                break;
                            }
                        }
                    }
                    _ = &mut timeout => {
                        eprintln!("⏰ Timeout — no credentials received in 60s");
                        break;
                    }
                }
            }
            eprintln!("Done.");
        }
        Err(e) => {
            eprintln!("❌ Discovery failed: {e:?}");
        }
    }
}
