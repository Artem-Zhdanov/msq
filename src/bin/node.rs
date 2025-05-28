use anyhow::Result;
use clap::Parser;
use msq::{
    MAGIC_NUMBER,
    config::{CliArgs, Config, read_yaml},
    get_credential,
    metrics::{Metrics, init_metrics},
    now_ms,
};
use opentelemetry::KeyValue;
use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::time::sleep;
use transport_layer::{
    NetConnection, NetIncomingRequest, NetListener, NetRecvRequest, NetTransport,
    msquic::{MsQuicNetConnection, MsQuicTransport},
};

const TAG_ACCEPT: &str = "accept";
const TAG_CONN: &str = "conn";
const TAG_RCV: &str = "recv";
const TAG_SEND: &str = "send";
const TAG_DECODE: &str = "decode";
const TAG_MISSMATCH: &str = "mismatch";
const TAG_FATAL: &str = "fatal";

#[tokio::main]
async fn main() {
    let block_size = std::env::var("BLOCK_SIZE_KB")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(50)
        * 1024;

    let port = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(5001);

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

    let sub_handle = tokio::spawn(run_subscriber(metrics.clone(), port));

    let _ = tokio::spawn(run_publishers(
        metrics.clone(),
        config.ips,
        port,
        block_size,
    ));

    match sub_handle.await {
        Ok(Err(error)) => {
            tracing::error!("{error:?}");
            metrics
                .errors
                .add(1, &[KeyValue::new("tag", TAG_FATAL.to_string())]);
        }
        _ => {}
    }

    unsafe {
        libc::_exit(1);
    }
}

async fn run_publishers(
    metrics: Arc<Metrics>,
    ips: Vec<String>,
    port: u16,
    block_size: usize,
) -> Result<()> {
    let len = ips.len();
    let transport = MsQuicTransport::new();

    for ip in ips {
        let socket_address: SocketAddr = format!("{ip}:{port}")
            .parse()
            .expect("Can't parse ip or port");

        // Trying to establish conection with a peer until it is set
        loop {
            let metrics = metrics.clone();
            let connection = transport
                .connect(socket_address, &["xxx"], get_credential())
                .await;

            match connection {
                Ok(conn) => {
                    // This thread never panics/exits
                    let _ = tokio::spawn(pubslish(metrics, conn, block_size));
                    sleep(Duration::from_secs(1)).await;
                    break;
                }
                Err(err) => {
                    tracing::error!("Can't connect to ip {ip}: {err}");
                    metrics
                        .errors
                        .add(1, &[KeyValue::new("tag", TAG_CONN.to_string())]);
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
    tracing::info!("All {len} publishers run.");
    Ok(())
}

async fn pubslish(
    metrics: Arc<Metrics>,
    connection: MsQuicNetConnection,
    block_size: usize,
) -> Result<()> {
    tracing::info!("Sending to {}", connection.remote_addr());

    loop {
        let moment = Instant::now();
        match connection.send(&create_message(block_size)).await {
            Ok(_) => tracing::info!("Sent"),
            Err(err) => {
                tracing::error!("{err:?}");
                metrics
                    .errors
                    .add(1, &[KeyValue::new("tag", TAG_SEND.to_string())]);
            }
        }
        let elapsed = moment.elapsed();
        let pause = Duration::from_millis(330).saturating_sub(elapsed);
        tracing::debug!("Pause is {:?}", pause);
        metrics.send_time.record(elapsed.as_millis() as u64, &[]);
        tokio::time::sleep(pause).await;
    }
}

fn create_message(size: usize) -> Vec<u8> {
    let mut message = vec![42u8; size];
    message[0..8].copy_from_slice(&MAGIC_NUMBER.to_be_bytes());
    message[8..16].copy_from_slice(&now_ms().to_be_bytes());
    message
}

async fn run_subscriber(metrics: Arc<Metrics>, port: u16) -> Result<()> {
    let transport = MsQuicTransport::new();
    let socket_address: SocketAddr = format!("0.0.0.0:{port}")
        .parse()
        .expect("Can't parse port number");

    let listener = transport
        .create_listener(socket_address, &["xxx"], get_credential())
        .await
        .expect("Check setting!");

    loop {
        let incoming_request = listener
            .accept()
            .await
            .expect("The whole process will be canceled if incoming request can't be accepted");

        let metrics = metrics.clone();
        let _ = tokio::spawn(async move {
            let connection = match incoming_request.accept().await {
                Ok(conn) => conn,
                Err(error) => {
                    tracing::error!("{error:?}");
                    metrics
                        .errors
                        .add(1, &[KeyValue::new("tag", TAG_ACCEPT.to_string())]);
                    return;
                }
            };
            loop {
                let message = connection.accept_recv().await;

                let message = match message {
                    Ok(message) => message.recv().await,
                    Err(error) => {
                        tracing::error!("{error:?}");
                        metrics
                            .errors
                            .add(1, &[KeyValue::new("tag", TAG_RCV.to_string())]);
                        break;
                    }
                };

                match message {
                    Ok(message) => {
                        // Check message consistency
                        let magic =
                            u64::from_be_bytes(message[..8].try_into().expect("Can't fail"));

                        if magic != MAGIC_NUMBER {
                            tracing::error!("Protocol mismatch!");
                            metrics
                                .errors
                                .add(1, &[KeyValue::new("tag", TAG_MISSMATCH.to_string())]);
                        } else {
                            let sent_ts =
                                u64::from_be_bytes(message[8..16].try_into().expect("Can't fail"));
                            let latency = now_ms().saturating_sub(sent_ts);
                            tracing::info!(
                                "Got message, len {}, latency: {}",
                                message.len(),
                                latency
                            );
                            metrics.latency.record(latency, &[]);
                        }
                    }
                    Err(error) => {
                        tracing::error!("Error: {:?}", error);
                        metrics
                            .errors
                            .add(1, &[KeyValue::new("tag", TAG_DECODE.to_string())]);
                    }
                }
            }
        });
    }
}
