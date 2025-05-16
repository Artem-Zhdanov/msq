use anyhow::Result;
use clap::Parser;
use msq::{
    MAGIC_NUMBER,
    config::{CliArgs, Config, Subscriber, read_yaml},
    get_credential,
    metrics::{Metrics, init_metrics},
    now_ms, ports_string_to_vec,
};
use std::{net::SocketAddr, sync::Arc};
use transport_layer::{
    NetConnection, NetIncomingRequest, NetListener, NetTransport, msquic::MsQuicTransport,
};

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

    // Run subscribers
    for Subscriber { address, ports } in config.subscriber {
        for port in ports_string_to_vec(&ports).unwrap() {
            let metrics = metrics.clone();
            let address = address.clone();
            let _ = tokio::spawn(run_server(metrics, address, port));
        }
    }

    tokio::signal::ctrl_c().await.unwrap();
    unsafe {
        libc::_exit(0);
    }
}

async fn run_server(metrics: Arc<Metrics>, address: String, port: u16) -> Result<()> {
    let transport = MsQuicTransport::new();
    let socket_address: SocketAddr = format!("{address}:{port}").parse().unwrap();

    let listener = transport
        .create_listener(socket_address, &["xxx"], get_credential())
        .await
        .expect("Check setting!");

    let incoming_request = listener.accept().await.unwrap();
    let msq_conn = incoming_request.accept().await.unwrap();

    loop {
        match msq_conn.recv().await {
            Ok(message) => {
                // Check message consistency
                let magic = u64::from_be_bytes(message[4..8 + 4].try_into()?);
                if magic != MAGIC_NUMBER {
                    tracing::error!("Protocol mismatch!");
                } else {
                    let sent_ts = u64::from_be_bytes(message[8 + 4..16 + 4].try_into()?);
                    let latency = now_ms().saturating_sub(sent_ts);
                    tracing::info!("Got message, len {}, latency: {}", message.len(), latency);
                    metrics.latency.record(latency, &[]);
                }
            }
            Err(error) => {
                eprint!("Error: {:?}", error);
            }
        }
    }
    // Ok(())
}
