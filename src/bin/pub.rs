use anyhow::Result;
use clap::Parser;
use msq::config::{BLOCK_SIZE, CliArgs, Config, Peers, read_yaml};
use msq::metrics::{Metrics, init_metrics};
use msq::quic_settings::get_quic_settings;
use msq::{MAGIC_NUMBER, now_ms, ports_string_to_vec};
use msquic_async::Connection;
use msquic_async::msquic::{
    Api, BufferRef, Configuration, CredentialConfig, CredentialFlags, Registration,
    RegistrationConfig,
};
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::io::AsyncWriteExt;
use tokio::runtime::Runtime;

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

        for (i, port) in ports.into_iter().enumerate() {
            // Distribute threads starting time
            std::thread::sleep(Duration::from_micros(i as u64 * delta));
            let metrics = metrics.clone();
            let addr_peer = addr_peer.clone();
            let _ = std::thread::spawn(move || {
                let peer_addr = addr_peer.clone();
                let metrics_clone = metrics.clone();

                let reg = Registration::new(&RegistrationConfig::default()).unwrap();
                let alpn = [BufferRef::from("qtest")];
                let settings = get_quic_settings();
                let configuration = Configuration::new(&reg, &alpn, Some(&settings)).unwrap();

                let cred_config = CredentialConfig::new_client()
                    .set_credential_flags(CredentialFlags::NO_CERTIFICATE_VALIDATION);

                configuration.load_credential(&cred_config).unwrap();

                let conn = msquic_async::Connection::new(&reg).unwrap();

                // This is just a test, I'm not going to handle publisher faults.
                // We will see it in metrics as reduced number of messages sent
                let rt = Runtime::new().unwrap();
                rt.block_on(start_client(
                    conn,
                    configuration,
                    peer_addr,
                    port,
                    metrics_clone,
                ))
                .unwrap();
            });
        }
    }

    tokio::signal::ctrl_c().await.unwrap();
    unsafe {
        libc::_exit(0);
    }
}

async fn start_client(
    conn: Connection,
    configuration: Configuration,
    peer_addr: String,
    port: u16,
    _metrics: Arc<Metrics>,
) -> Result<()> {
    conn.start(&configuration, &peer_addr, port).await.unwrap();
    loop {
        let mut stream = conn
            .open_outbound_stream(msquic_async::StreamType::Unidirectional, false)
            .await?;
        let mut data_to_send = vec![42u8; BLOCK_SIZE];
        data_to_send[0..8].copy_from_slice(&MAGIC_NUMBER.to_be_bytes());
        data_to_send[8..16].copy_from_slice(&now_ms().to_be_bytes());
        let moment = Instant::now();
        stream.write_all(&data_to_send).await?;
        stream.flush().await?;

        let pause = Duration::from_millis(300).saturating_sub(moment.elapsed());
        tracing::info!("Pause is {:?}", pause);
        tokio::time::sleep(pause).await;

        // tracing::info!("Perf {:#?}", Api::get_perf());
    }
}
