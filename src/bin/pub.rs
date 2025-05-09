use std::sync::Arc;
use std::{ffi::c_void, thread::sleep, time::Duration};

use anyhow::Result;
use clap::Parser;
use msq::config::{CliArgs, Config, Peers, read_yaml};
use msq::metrics::{Metrics, init_metrics};
use msq::{MAGIC_NUMBER, now_ms, ports_string_to_vec};
use msquic::{
    BufferRef, Configuration, Connection, ConnectionEvent, ConnectionRef, ConnectionShutdownFlags,
    CredentialConfig, CredentialFlags, Registration, RegistrationConfig, SendFlags, Settings,
    Status, Stream, StreamEvent, StreamOpenFlags, StreamRef, StreamStartFlags,
};

const BLOCK_SIZE: usize = 1024 * 1024;

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
    let mut _handles: Vec<std::thread::JoinHandle<()>> = vec![];
    for Peers { addr_peer, ports } in config.publisher {
        let ports = ports_string_to_vec(&ports).unwrap();
        let delta = 330000 / ports.len() as u64;

        for (i, port) in ports.into_iter().enumerate() {
            let peer_addr = addr_peer.clone();
            let metrics_clone = metrics.clone();
            let delay = Duration::from_micros(i as u64 * delta);

            let h = std::thread::spawn(move || {
                std::thread::sleep(delay);
                if let Err(err) = start_client(peer_addr, port, metrics_clone) {
                    tracing::error!("Publisher task failed: {}", err);
                }
            });
            _handles.push(h);
        }
    }

    tokio::signal::ctrl_c().await.unwrap();
    unsafe {
        libc::_exit(0);
    }
}

fn start_client(peer_addr: String, port: u16, _metrics: Arc<Metrics>) -> Result<()> {
    let reg = Registration::new(&RegistrationConfig::default()).unwrap();
    let alpn = [BufferRef::from("qtest")];

    let client_settings = Settings::new().set_IdleTimeoutMs(100000);

    let client_config = Configuration::open(&reg, &alpn, Some(&client_settings)).unwrap();
    {
        let cred_config = CredentialConfig::new_client()
            .set_credential_flags(CredentialFlags::NO_CERTIFICATE_VALIDATION);
        client_config.load_credential(&cred_config).unwrap();
    }

    let conn_handler = move |conn: ConnectionRef, ev: ConnectionEvent| {
        tracing::info!("Client connection event: {ev:?}");
        match ev {
            ConnectionEvent::Connected { .. } => {
                println!("Sent..");
                for _ in 0..20 {
                    if let Err(status) = open_stream_and_send(&conn) {
                        tracing::error!("Client send failed with status {status}");
                        conn.shutdown(ConnectionShutdownFlags::NONE, 0);
                    } else {
                        tracing::info!("sent..");
                    }
                    sleep(Duration::from_millis(100));
                }
            }
            ConnectionEvent::ShutdownComplete { .. } => {
                // No need to close. Main function owns the handle.
            }
            _ => {}
        };
        Ok(())
    };

    tracing::info!("open client connection");
    let conn = Connection::open(&reg, conn_handler).unwrap();

    conn.start(&client_config, &peer_addr, port).unwrap();

    std::thread::park();
    Ok(())
}

fn stream_handler(stream: StreamRef, ev: StreamEvent) -> Result<(), Status> {
    tracing::info!("Client stream event: {ev:?}");
    match ev {
        StreamEvent::StartComplete { id, .. } => {
            assert_eq!(stream.get_stream_id().unwrap(), id);
        }
        StreamEvent::SendComplete {
            cancelled: _,
            client_context,
        } => {
            let _ = unsafe { Box::from_raw(client_context as *mut (Vec<u8>, Box<[BufferRef; 1]>)) };
        }

        StreamEvent::ShutdownComplete { .. } => {
            let _ = unsafe { Stream::from_raw(stream.as_raw()) };
        }
        _ => {}
    }
    Ok(())
}

fn open_stream_and_send(conn: &ConnectionRef) -> Result<(), Status> {
    let s = Stream::open(&conn, StreamOpenFlags::UNIDIRECTIONAL, stream_handler)?;
    s.start(StreamStartFlags::NONE)?;
    let mut data_to_send = vec![42u8; BLOCK_SIZE];

    // BufferRef needs to be heap allocated

    data_to_send[0..8].copy_from_slice(&MAGIC_NUMBER.to_be_bytes());
    data_to_send[8..16].copy_from_slice(&now_ms().to_be_bytes());

    let b_ref = Box::new([BufferRef::from((*data_to_send).as_ref() as &[u8])]);
    // let b_ref = Box::new([BufferRef::from((*b).as_ref())]);

    let ctx = Box::new((data_to_send, b_ref));
    unsafe {
        s.send(
            ctx.1.as_slice(),
            SendFlags::FIN,
            ctx.as_ref() as *const _ as *const c_void,
        )
    }?;
    // detach the buffer
    let _ = Box::into_raw(ctx);
    // detach stream and let callback cleanup
    unsafe { s.into_raw() };
    Ok::<(), Status>(())
}
//
//
//
// let open_stream_and_send = || {
//     let s =
//         Stream::open(&conn, StreamOpenFlags::UNIDIRECTIONAL, stream_handler)?;
//     s.start(StreamStartFlags::NONE)?;
//     // BufferRef needs to be heap allocated
//     let mut data_to_send = vec![42u8; BLOCK_SIZE];

//     data_to_send[0..8].copy_from_slice(&MAGIC_NUMBER.to_be_bytes());
//     data_to_send[8..16].copy_from_slice(&now_ms().to_be_bytes());

//     let b_ref = Box::new([BufferRef::from((*data_to_send).as_ref() as &[u8])]);
//     // let b_ref = Box::new([BufferRef::from((*b).as_ref())]);

//     let ctx = Box::new((data_to_send, b_ref));
//     unsafe {
//         s.send(
//             ctx.1.as_slice(),
//             SendFlags::FIN,
//             ctx.as_ref() as *const _ as *const c_void,
//         )
//     }?;
//     // detach the buffer
//     let _ = Box::into_raw(ctx);
//     // detach stream and let callback cleanup
//     unsafe { s.into_raw() };
//     Ok::<(), Status>(())
// };
