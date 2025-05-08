use std::{net::SocketAddr, sync::Arc, time::Instant};

use anyhow::Result;
use clap::Parser;
use msq::{
    MAGIC_NUMBER,
    config::{CliArgs, Config, Subscriber, read_yaml},
    get_test_cred,
    metrics::{Metrics, init_metrics},
    now_ms, ports_string_to_vec,
};
use msquic::{
    Addr, BufferRef, Configuration, Connection, ConnectionEvent, ConnectionRef, CredentialConfig,
    CredentialFlags, Listener, ListenerEvent, Registration, RegistrationConfig,
    ServerResumptionLevel, Settings, Stream, StreamEvent, StreamRef,
};

#[derive(Debug)]
enum ReceivedData {
    Data(Vec<u8>),
    Fin,
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
                println!("Running servers (subscribers)");

                if let Err(err) = start_server(addr_clone, port, metrics_clone) {
                    tracing::error!("Subscriber error: {}", err);
                }
            });
        }
    }

    tokio::signal::ctrl_c().await.unwrap();
    eprintln!("Ctrl+C pressed, exiting.");
    std::process::exit(0);
}

fn start_server(address: String, port: u16, metrics: Arc<Metrics>) -> Result<()> {
    let cred = get_test_cred();

    let reg = Registration::new(&RegistrationConfig::default()).unwrap();
    let alpn = [BufferRef::from("qtest")];
    let settings = Settings::new()
        .set_ServerResumptionLevel(ServerResumptionLevel::ResumeAndZerortt)
        .set_PeerBidiStreamCount(100)
        .set_PeerUnidiStreamCount(100);

    let config = Configuration::open(&reg, &alpn, Some(&settings)).unwrap();

    let cred_config = CredentialConfig::new()
        .set_credential_flags(CredentialFlags::NO_CERTIFICATE_VALIDATION)
        .set_credential(cred);

    config.load_credential(&cred_config).unwrap();
    let config = Arc::new(config);
    let config_clone = config.clone();

    let (s_tx, s_rx) = std::sync::mpsc::channel::<ReceivedData>();

    let stream_handler = move |stream: StreamRef, ev: StreamEvent| {
        println!("Stream event: {ev:?}");
        match ev {
            StreamEvent::Receive {
                absolute_offset: _,
                total_buffer_length: _,
                buffers,
                flags: _,
            } => {
                // Send the result to main thread.
                let bytes = buffers_as_bytes(buffers);
                tracing::info!("AAA Received");
                if let Err(err) = s_tx.send(ReceivedData::Data(bytes)) {
                    tracing::error!("mpsc error 1: {:?}", err);
                } else {
                    tracing::info!("Sent data");
                }
            }
            StreamEvent::PeerSendShutdown { .. } => {
                tracing::info!("AAA Peer sent stream shutdown");
            }
            StreamEvent::SendComplete {
                cancelled: _,
                client_context,
            } => unsafe {
                tracing::info!("Send complete");
                let _ = Box::from_raw(client_context as *mut (Vec<u8>, Box<[BufferRef; 1]>));
            },
            StreamEvent::ShutdownComplete { .. } => {
                // auto close
                unsafe { Stream::from_raw(stream.as_raw()) };
                tracing::info!("AAA Shutdown complete");
                if let Err(err) = s_tx.send(ReceivedData::Fin) {
                    tracing::error!("mpsc error 2: {:?}", err);
                } else {
                    tracing::info!("Sent fin");
                }
            }
            _ => {}
        };
        Ok(())
    };

    let conn_handler = move |conn: ConnectionRef, ev: ConnectionEvent| {
        println!("Connection event: {ev:?}");
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
        println!("Listener event: {ev:?}");
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
    println!("Started listener on {:?}", local_address);

    // let mut total_bytes = 0;
    // let mut moment: Option<Instant> = None;

    let mut block_agg = vec![];
    while let Ok(received_data) = s_rx.recv() {
        match received_data {
            ReceivedData::Data(mut v) => {
                tracing::info!("RCVD: bytes arr len {}", v.len());
                block_agg.append(&mut v);
            }
            ReceivedData::Fin => {
                tracing::info!("RCVD FIN");

                let magic = u64::from_be_bytes(block_agg[..8].try_into()?);
                assert_eq!(magic, MAGIC_NUMBER, "Protocol mismatch!");

                let sent_ts = u64::from_be_bytes(block_agg[8..16].try_into()?);
                let latency = now_ms() - sent_ts;
                metrics.latency.record(latency, &[]);
                tracing::info!("Latency ms: {latency}");
                block_agg.clear();
            }
        }

        //     if moment.is_none() {
        //         moment = Some(Instant::now());
        //     }
        //     total_bytes += bytes_received;
        //     if total_bytes > 1024 * 1024 {
        //         let ms = moment.unwrap().elapsed().as_millis();
        //         let speed = (total_bytes / ms as usize) * 1_000 / 1024 / 1024;

        //         println!(
        //             "total bytes {} elapsed {} ms,  speed {} MByte ",
        //             total_bytes, ms, speed
        //         );
        //         moment = None;
        //         total_bytes = 0;
        //     }
    }
    Ok(())
}

fn buffers_as_bytes(buffers: &[BufferRef]) -> Vec<u8> {
    let mut v = Vec::new();
    for b in buffers {
        v.extend_from_slice(b.as_bytes());
    }
    v
}
