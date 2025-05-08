use std::{ffi::c_void, thread::sleep, time::Duration};
use std::{net::SocketAddr, sync::Arc, time::Instant};

use anyhow::Result;
use clap::Parser;
use msq::config::{CliArgs, Config, Peers, read_yaml};
use msq::metrics::{Metrics, init_metrics};
use msq::ports_string_to_vec;
use msquic::{
    BufferRef, Configuration, Connection, ConnectionEvent, ConnectionRef, ConnectionShutdownFlags,
    CredentialConfig, CredentialFlags, Registration, RegistrationConfig, SendFlags, Settings,
    Status, Stream, StreamEvent, StreamOpenFlags, StreamRef, StreamStartFlags,
};

const BLOCK_SIZE: usize = 1024 * 1024;
fn main() {
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
        let jitter = 330000 / ports.len() as u64;

        for (i, port) in ports.into_iter().enumerate() {
            let peer_addr = addr_peer.clone();
            let listen_addr = config.my_address.clone();
            let metrics_clone = metrics.clone();
            let delay = Duration::from_micros(i as u64 * jitter);

            let _ = std::thread::spawn(move || {
                println!("Running publisher");
                std::thread::sleep(delay);
                if let Err(err) = start_client(listen_addr, peer_addr, port, metrics_clone) {
                    tracing::error!("Publisher task failed: {}", err);
                }
            });
        }
    }
}

fn start_client(
    listen_addr: String,
    peer_addr: String,
    port: u16,
    metrics: Arc<Metrics>,
) -> Result<()> {
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
        println!("Client connection event: {ev:?}");
        match ev {
            ConnectionEvent::Connected { .. } => {
                for _ in 0..10 {
                    if open_stream_and_send(&conn).is_err() {
                        println!("Client send failed");
                        conn.shutdown(ConnectionShutdownFlags::NONE, 0);
                    }
                    sleep(Duration::from_millis(200));
                    println!("Sent");
                }
            }
            ConnectionEvent::ShutdownComplete { .. } => {
                // No need to close. Main function owns the handle.
            }
            _ => {
                println!("@@@");
            }
        };
        Ok(())
    };

    println!("open client connection");
    let conn = Connection::open(&reg, conn_handler).unwrap();

    conn.start(&client_config, &peer_addr, port).unwrap();

    sleep(Duration::from_secs(1000000));
    Ok(())
}

fn stream_handler(stream: StreamRef, ev: StreamEvent) -> Result<(), Status> {
    println!("Client stream event: {ev:?}");
    match ev {
        StreamEvent::StartComplete { id, .. } => {
            assert_eq!(stream.get_stream_id().unwrap(), id);
        }
        StreamEvent::SendComplete {
            cancelled,
            client_context,
        } => {
            println!("cancelled {}", cancelled);
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
    // BufferRef needs to be heap allocated
    let b = vec![42u8; BLOCK_SIZE];
    let b_ref = Box::new([BufferRef::from((*b).as_ref() as &[u8])]);
    // let b_ref = Box::new([BufferRef::from((*b).as_ref())]);

    let ctx = Box::new((b, b_ref));
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
