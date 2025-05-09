use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use anyhow::Result;
use clap::Parser;
use msq::{
    MAGIC_NUMBER,
    config::{CliArgs, Config, Subscriber, read_yaml},
    get_test_cred,
    metrics::{Metrics, init_metrics},
    now_ms, ports_string_to_vec,
    quic_settings::get_quic_settings,
};
use msquic::{
    Addr, BufferRef, Configuration, Connection, ConnectionEvent, ConnectionRef, CredentialConfig,
    CredentialFlags, Listener, ListenerEvent, Registration, RegistrationConfig,
    ServerResumptionLevel, Settings, Stream, StreamEvent, StreamRef,
};

type StreamId = u64;

#[derive(Debug)]
enum ReceivedData {
    Data((StreamId, Vec<u8>)),
    Fin(StreamId),
}

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
            let metrics_clone = metrics.clone();
            let addr_clone = address.clone();
            let _ = std::thread::spawn(move || {
                tracing::info!("Running servers (subscribers)");

                if let Err(err) = start_server(addr_clone, port, metrics_clone) {
                    tracing::error!("Subscriber error: {}", err);
                }
            });
        }
    }

    tokio::signal::ctrl_c().await.unwrap();
    unsafe {
        libc::_exit(0);
    }
}

fn start_server(address: String, port: u16, metrics: Arc<Metrics>) -> Result<()> {
    let cred = get_test_cred();

    let reg = Registration::new(&RegistrationConfig::default()).unwrap();
    let alpn = [BufferRef::from("qtest")];
    let settings = get_quic_settings();

    // let settings = Settings::new()
    //     .set_ServerResumptionLevel(ServerResumptionLevel::ResumeAndZerortt)
    //     .set_PeerBidiStreamCount(10)
    //     .set_PeerUnidiStreamCount(1000);

    let config = Configuration::open(&reg, &alpn, Some(&settings)).unwrap();

    let cred_config = CredentialConfig::new()
        .set_credential_flags(CredentialFlags::NO_CERTIFICATE_VALIDATION)
        .set_credential(cred);

    config.load_credential(&cred_config).unwrap();
    let config = Arc::new(config);
    let config_clone = config.clone();

    let (s_tx, s_rx) = std::sync::mpsc::channel::<ReceivedData>();

    let stream_handler = move |stream: StreamRef, ev: StreamEvent| {
        // tracing::info!("Stream event: {ev:?}");
        match ev {
            StreamEvent::Receive {
                absolute_offset: _,
                total_buffer_length: _,
                buffers,
                flags: _,
            } => {
                // Send results to the main thread.
                match stream.get_stream_id() {
                    Ok(stream_id) => {
                        let bytes = buffers_as_bytes(buffers);
                        if let Err(err) = s_tx.send(ReceivedData::Data((stream_id, bytes))) {
                            tracing::error!("mpsc error 1: {:?}", err);
                        }
                    }
                    Err(status) => tracing::error!(
                        "StreamEvent::Receive, can't get stream_id, status {:?}",
                        status
                    ),
                }
            }
            StreamEvent::PeerSendShutdown { .. } => {}
            StreamEvent::SendComplete {
                cancelled: _,
                client_context,
            } => unsafe {
                let _ = Box::from_raw(client_context as *mut (Vec<u8>, Box<[BufferRef; 1]>));
            },
            StreamEvent::ShutdownComplete { .. } => {
                // Send stream FIN to the main thread.
                match stream.get_stream_id() {
                    Ok(stream_id) => {
                        if let Err(err) = s_tx.send(ReceivedData::Fin(stream_id)) {
                            tracing::error!("mpsc error 2: {:?}", err);
                        }
                    }
                    Err(status) => tracing::error!(
                        "StreamEvent::ShutdownComplete, can't get stream_id, status {:?}",
                        status
                    ),
                }
                // auto close
                unsafe { Stream::from_raw(stream.as_raw()) };
            }
            _ => {}
        };
        Ok(())
    };

    let conn_handler = move |conn: ConnectionRef, ev: ConnectionEvent| {
        // tracing::info!("Connection event: {ev:?}");
        match ev {
            ConnectionEvent::PeerStreamStarted { stream, flags: _ } => {
                stream.set_callback_handler(stream_handler.clone());
            }
            ConnectionEvent::ShutdownComplete { .. } => {
                // auto close connection
                unsafe { Connection::from_raw(conn.as_raw()) };
            }
            _ => {}
        };
        Ok(())
    };

    let l = Listener::open(&reg, move |_, ev| {
        tracing::info!("Listener event: {ev:?}");
        match ev {
            ListenerEvent::NewConnection {
                info: _,
                connection,
            } => {
                connection.set_callback_handler(conn_handler.clone());
                connection.set_configuration(&config_clone)?;
            }
            ListenerEvent::StopComplete {
                app_close_in_progress: _,
            } => {}
        }
        Ok(())
    })
    .unwrap();

    let local_address = Addr::from(SocketAddr::new(address.parse().unwrap(), port));
    l.start(&alpn, Some(&local_address)).unwrap();
    tracing::info!("Started listener on {:?}", local_address);

    let mut block_agg: HashMap<StreamId, Vec<u8>> = HashMap::new();
    while let Ok(received_data) = s_rx.recv() {
        match received_data {
            ReceivedData::Data((stream_id, bytes)) => {
                // tracing::info!("RCVD: bytes arr len {}", bytes.len());
                block_agg
                    .entry(stream_id)
                    .or_default()
                    .extend_from_slice(&bytes);
            }
            ReceivedData::Fin(stream_id) => {
                // tracing::info!("RCVD FIN");
                if let Some(data) = block_agg.remove(&stream_id) {
                    let magic = u64::from_be_bytes(data[..8].try_into()?);
                    if magic != MAGIC_NUMBER {
                        tracing::error!("Protocol mismatch!");
                    } else {
                        let sent_ts = u64::from_be_bytes(data[8..16].try_into()?);
                        let latency = now_ms().saturating_sub(sent_ts);
                        metrics.latency.record(latency, &[]);
                        tracing::info!("Latency ms: {latency}");
                    }

                    block_agg.clear();
                } else {
                    tracing::error!("Received FIN for stream {stream_id}, wich data not found");
                }
            }
        }
    }
    tracing::error!("Thread is going to exit!");

    Ok(())
}

fn buffers_as_bytes(buffers: &[BufferRef]) -> Vec<u8> {
    let mut v = Vec::new();
    for b in buffers {
        v.extend_from_slice(b.as_bytes());
    }
    v
}
