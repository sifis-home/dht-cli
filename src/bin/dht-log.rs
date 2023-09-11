use futures::prelude::*;
use sifis_config::ConfigParser;
use sifis_dht::cache::Builder;
use sifis_dht::Config;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let cfg = ConfigParser::<Config>::new()
        .with_config_path(".config/domo/dht-cli.toml")
        .parse();

    let (dht, events) = Builder::from_config(cfg).make_channel().await?;

    println!("Logging from hash {:x}", dht.get_hash().await);

    let log = events.for_each(|ev| {
        println!("{ev:?}");
        future::ready(())
    });

    log.await;

    Ok(())
}
