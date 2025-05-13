use anyhow::Result;
use clap::Parser;
use msq::{
    MAGIC_NUMBER,
    config::{BLOCK_SIZE, CliArgs, Config, Subscriber, read_yaml},
    get_test_cred,
    metrics::{Metrics, init_metrics},
    now_ms, ports_string_to_vec,
    quic_settings::get_quic_settings,
};
use msquic_async::{
    Listener,
    msquic::{
        BufferRef, Configuration, CredentialConfig, CredentialFlags, Registration,
        RegistrationConfig,
    },
};
use tokio::runtime::Runtime;

use std::{net::SocketAddr, sync::Arc};
use tokio::io::AsyncReadExt;

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
            let _ = std::thread::spawn(move || {
                let metrics_clone = metrics.clone();
                let cred = get_test_cred();
                let reg = Registration::new(&RegistrationConfig::default()).unwrap();
                let alpn: [BufferRef; 1] = [BufferRef::from("qtest")];
                let settings = get_quic_settings();
                let config = Configuration::new(&reg, &alpn, Some(&settings)).unwrap();
                let cred_config = CredentialConfig::new()
                    .set_credential_flags(CredentialFlags::NO_CERTIFICATE_VALIDATION)
                    .set_credential(cred);

                config.load_credential(&cred_config).unwrap();

                let l = msquic_async::Listener::new(&reg, config).unwrap();
                let local_address = SocketAddr::new(address.parse().unwrap(), port);
                l.start(&alpn, Some(local_address)).unwrap();
                tracing::info!("Started listener on {:?}", local_address);

                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                // let _ = tokio::spawn(async move { start_server(l, metrics_clone).await });
                rt.block_on(start_server(l, metrics_clone)).unwrap();
            });
        }
    }

    tokio::signal::ctrl_c().await.unwrap();
    unsafe {
        libc::_exit(0);
    }
}

async fn start_server(l: Listener, metrics: Arc<Metrics>) -> Result<()> {
    while let Ok(conn) = l.accept().await {
        let metrics_clone = metrics.clone();

        tracing::info!("new connection established");
        tokio::spawn(async move {
            loop {
                match conn.accept_inbound_uni_stream().await {
                    Ok(mut stream) => {
                        let mut block_agg = vec![];
                        loop {
                            let mut buf = [0u8; 500 * 1024];

                            let len = stream.read(&mut buf).await?;
                            if len > 0 {
                                block_agg.extend_from_slice(&buf[..len]);
                            }
                            if block_agg.len() == BLOCK_SIZE {
                                let magic = u64::from_be_bytes(block_agg[..8].try_into()?);
                                if magic != MAGIC_NUMBER {
                                    tracing::error!("Protocol mismatch!");
                                } else {
                                    let sent_ts = u64::from_be_bytes(block_agg[8..16].try_into()?);
                                    let latency = now_ms().saturating_sub(sent_ts);
                                    metrics_clone.latency.record(latency, &[]);

                                    tracing::info!(
                                        "read  from stream {}, latency ms: {} ",
                                        stream.id().unwrap(),
                                        latency
                                    );
                                    break;
                                }
                            }
                        }
                    }
                    Err(err) => {
                        tracing::error!("error on accept {}", err);
                        break;
                    }
                }
            }
            Ok::<_, anyhow::Error>(())
        });
    }
    Ok(())
}
