use anyhow::Result;
use clap::Parser;
use msq::config::{BLOCK_SIZE, CliArgs, Config, Peers, read_yaml};
use msq::metrics::{Metrics, init_metrics};

use msq::{MAGIC_NUMBER, get_credential, now_ms, ports_string_to_vec};

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use transport_layer::msquic::MsQuicTransport;
use transport_layer::*;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_ansi(false)
        .init();

    let cli: CliArgs = CliArgs::parse();
    let config = match read_yaml::<Config>(&cli.config) {
        Ok(config) => config,
        Err(error) => {
            tracing::error!("Error parsing config file {:?}: {:?}", cli.config, error);
            std::process::exit(1);
        }
    };

    let metrics = init_metrics();

    // Run publisher
    for Peers { addr_peer, ports } in config.publisher {
        let ports = ports_string_to_vec(&ports).unwrap();
        let delta = 330000 / ports.len() as u64;

        for port in ports {
            // Distribute threads starting time
            tokio::time::sleep(Duration::from_micros(delta)).await;
            let metrics = metrics.clone();
            let addr_peer = addr_peer.clone();
            let _ = tokio::spawn(run_client(metrics, addr_peer, port));
        }
    }

    tokio::signal::ctrl_c().await.unwrap();
    unsafe {
        libc::_exit(0);
    }
}

async fn run_client(_metrics: Arc<Metrics>, address: String, port: u16) -> Result<()> {
    let socket_address: SocketAddr = format!("{address}:{port}").parse().unwrap();

    let transport = MsQuicTransport::new();
    let connection = transport
        .connect(socket_address, &["xxx"], get_credential())
        .await
        .expect("Check setting!");

    loop {
        let moment = Instant::now();
        match connection.send(&create_message()).await {
            Ok(_) => tracing::info!("Sent"),
            Err(error) => {
                tracing::error!("{:?}", error)
            }
        }
        let pause = Duration::from_millis(330).saturating_sub(moment.elapsed());
        tracing::info!("Pause is {:?}", pause);
        tokio::time::sleep(pause).await;
    }
}

fn create_message() -> Vec<u8> {
    let mut message = vec![42u8; BLOCK_SIZE];
    message[0..8].copy_from_slice(&MAGIC_NUMBER.to_be_bytes());
    message[8..16].copy_from_slice(&now_ms().to_be_bytes());
    message
}
