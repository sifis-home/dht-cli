use std::fmt::Write as _;

use sifis_config::ConfigParser;
use sifis_dht::cache::{Builder, Cache};
use sifis_dht::Config;

use reedline_repl_rs::clap::{value_parser, Arg, ArgMatches, Command};
use reedline_repl_rs::Repl;
use serde_json::Value;
use tokio_stream::StreamExt;
use tracing::debug;

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error(transparent)]
    Dht(#[from] sifis_dht::Error),
    #[error(transparent)]
    Repl(#[from] reedline_repl_rs::Error),
    #[error("Quit requested")]
    Quit,
}

async fn print_peers(_args: ArgMatches, dht: &mut Cache) -> Result<Option<String>, CliError> {
    let mut out: String = String::new();

    writeln!(out, "{:<15} {:<7} {:<5}", "Peer", "Hash", "Timestamp").unwrap();
    for peer in dht.peers().await {
        writeln!(
            out,
            "{:<15} {:x<7} {:<5}",
            peer.peer_id, peer.hash, peer.last_seen
        )
        .unwrap();
    }

    Ok(Some(out))
}

fn print_dht(_args: ArgMatches, _dht: &mut Cache) -> Result<Option<String>, CliError> {
    todo!("TBD");
    //    let out = serde_json::to_string_pretty(&dht.).unwrap();
    //    Ok(Some(out))
}

async fn send_volatile(args: ArgMatches, dht: &mut Cache) -> Result<Option<String>, CliError> {
    let v = args.get_one::<Value>("value").unwrap();

    dht.send(v.to_owned())?;

    Ok(None)
}

async fn send_persistent(args: ArgMatches, dht: &mut Cache) -> Result<Option<String>, CliError> {
    let topic = args.get_one::<String>("topic").unwrap();
    let uuid = args.get_one::<String>("uuid").unwrap();
    let v = args.get_one::<Value>("value").unwrap();

    dht.put(topic, uuid, v.to_owned()).await?;

    Ok(None)
}

async fn del_persistent(args: ArgMatches, dht: &mut Cache) -> Result<Option<String>, CliError> {
    let topic = args.get_one::<String>("topic").unwrap();
    let uuid = args.get_one::<String>("uuid").unwrap();

    dht.del(topic, uuid).await?;

    Ok(None)
}

async fn update_prompt(_context: &mut Cache) -> Result<Option<String>, CliError> {
    let msg = "Ok";
    Ok(Some(msg.to_owned()))
}

fn _setup_tracing(verbose: u8) {
    use tracing_subscriber::filter::EnvFilter;
    let filter: &str = match verbose {
        0 => "error",
        1 => "warn",
        2 => "info",
        3 => "debug",
        _ => "trace",
    };
    let filter: EnvFilter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(filter))
        .unwrap();

    tracing_subscriber::fmt().with_env_filter(filter).init()
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let cfg = ConfigParser::<Config>::new()
        .with_config_path(".config/domo/dht-cli.toml")
        .parse();

    let (dht, events) = Builder::from_config(cfg).make_channel().await?;

    let mut repl: Repl<Cache, CliError> = Repl::new(dht)
        .with_name("Sifis DHT REPL")
        .with_version("v0.1.0")
        .with_command_async(
            Command::new("hash").about("Print the current cache hash"),
            |_args, dht| {
                Box::pin(async { Ok(Some(format!("Current hash: {:x}", dht.get_hash().await))) })
            },
        )
        .with_command_async(
            Command::new("peers").about("Print the peers statistics"),
            |args, dht| Box::pin(print_peers(args, dht)),
        )
        .with_command(
            Command::new("dump").about("Dump the current dht state"),
            print_dht,
        )
        .with_command_async(
            Command::new("pub")
                .about("Publish a volatile message, it should be a JSON object")
                .arg(
                    Arg::new("value")
                        .value_parser(value_parser!(Value))
                        .required(true),
                ),
            |args, dht| Box::pin(send_volatile(args, dht)),
        )
        .with_command_async(
            Command::new("put")
                .about("Publish a persistent entry, the value should be a JSON object")
                .arg(
                    Arg::new("topic")
                        .value_parser(value_parser!(String))
                        .required(true),
                )
                .arg(
                    Arg::new("uuid")
                        .value_parser(value_parser!(String))
                        .required(true),
                )
                .arg(
                    Arg::new("value")
                        .value_parser(value_parser!(Value))
                        .required(true),
                ),
            |args, dht| Box::pin(send_persistent(args, dht)),
        )
        .with_command_async(
            Command::new("del")
                .about("Unpublish a persistent entry")
                .arg(
                    Arg::new("topic")
                        .value_parser(value_parser!(String))
                        .required(true),
                )
                .arg(
                    Arg::new("uuid")
                        .value_parser(value_parser!(String))
                        .required(true),
                ),
            |args, dht| Box::pin(del_persistent(args, dht)),
        )
        .with_command(
            Command::new("quit").about("Quit the repl"),
            |_, _context| Err(CliError::Quit),
        )
        .with_stop_on_ctrl_c(true)
        .with_on_after_command_async(|context| Box::pin(update_prompt(context)))
        .with_error_handler(|e, _context| {
            if matches!(e, CliError::Quit) {
                Err(reedline_repl_rs::Error::UnknownCommand("quit".to_string()))
            } else {
                eprintln!("{}", e);
                Ok(())
            }
        });

    use reedline_repl_rs::Error;

    tokio::task::spawn(async move {
        use sifis_dht::cache::Event;
        let mut events = std::pin::pin!(events);
        while let Some(ev) = events.next().await {
            match ev {
                Event::ReadyPeers(peers) => {
                    debug!("Peers ready {peers:?}");
                }
                Event::VolatileData(data) => {
                    debug!("Got data {data:#?}");
                }
                Event::PersistentData(elem) => {
                    debug!("Got elem {elem:#?}");
                }
            }
        }
    });

    match repl.run_async().await {
        Ok(_) => Ok(()),
        Err(Error::UnknownCommand(e)) => {
            if e == "quit" {
                Ok(())
            } else {
                Err(Error::UnknownCommand(e))
            }
        }
        Err(e) => Err(e),
    }?;

    Ok(())
}
