use std::{
    ffi::c_void,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Instant,
};

use msquic::{
    Addr, BufferRef, CertificateFile, Configuration, Connection, ConnectionEvent, ConnectionRef,
    ConnectionShutdownFlags, Credential, CredentialConfig, CredentialFlags, Listener,
    ListenerEvent, Registration, RegistrationConfig, SendFlags, ServerResumptionLevel, Settings,
    Status, Stream, StreamEvent, StreamOpenFlags, StreamRef, StreamShutdownFlags, StreamStartFlags,
};
const BLOCK_SIZE: usize = 1024 * 1024;

fn main() {
    let my_ip = "94.156.178.64"; // "94.156.25.224";
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

    let (s_tx, s_rx) = std::sync::mpsc::channel::<usize>();

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
                let s = buffers_to_string(buffers);
                println!("Received");
                s_tx.send(s).unwrap();
            }
            StreamEvent::PeerSendShutdown { .. } => {
                println!("Peer sent shutdown");
                // // reply to client
                // let b = "hello from server".as_bytes().to_vec();
                // let b_ref = Box::new([BufferRef::from((*b).as_ref())]);
                // let ctx = Box::new((b, b_ref));
                // if unsafe {
                //     stream.send(
                //         ctx.1.as_ref(),
                //         SendFlags::FIN,
                //         ctx.as_ref() as *const _ as *const c_void,
                //     )
                // }
                // .is_err()
                // {
                //     let _ = stream.shutdown(StreamShutdownFlags::ABORT, 0);
                // } else {
                //     // detach buffer
                //     let _ = Box::into_raw(ctx);
                // }
            }
            StreamEvent::SendComplete {
                cancelled: _,
                client_context,
            } => unsafe {
                println!("Send comlete");
                let _ = Box::from_raw(client_context as *mut (Vec<u8>, Box<[BufferRef; 1]>));
            },
            StreamEvent::ShutdownComplete { .. } => {
                // auto close
                unsafe { Stream::from_raw(stream.as_raw()) };
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

    println!("Starting listener");
    let local_address = Addr::from(SocketAddr::new(my_ip.parse().unwrap(), 4567));
    l.start(&alpn, Some(&local_address)).unwrap();

    let mut total_bytes = 0;
    let mut moment: Option<Instant> = None;

    while let Ok(bytes_received) = s_rx.recv() {
        if moment.is_none() {
            moment = Some(Instant::now());
        }
        total_bytes += bytes_received;
        if total_bytes == BLOCK_SIZE {
            let ms = moment.unwrap().elapsed().as_millis();
            let speed = (total_bytes / ms as usize) * 1_000 / 1024 / 1024;

            println!(
                "total bytes {} elapsed {} ms,  speed {} MByte ",
                total_bytes, ms, speed
            );
            moment = None;
            total_bytes = 0;
        }
    }

    l.stop();
}

pub fn get_test_cred() -> Credential {
    let cert_dir = std::env::temp_dir().join("msquic_test_rs");
    let key = "key.pem";
    let cert = "cert.pem";
    let key_path = cert_dir.join(key);
    let cert_path = cert_dir.join(cert);
    if !key_path.exists() || !cert_path.exists() {
        // remove the dir
        let _ = std::fs::remove_dir_all(&cert_dir);
        std::fs::create_dir_all(&cert_dir).expect("cannot create cert dir");
        // generate test cert using openssl cli
        let output = std::process::Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:4096",
                "-keyout",
                "key.pem",
                "-out",
                "cert.pem",
                "-sha256",
                "-days",
                "3650",
                "-nodes",
                "-subj",
                "/CN=localhost",
            ])
            .current_dir(cert_dir)
            .stderr(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .output()
            .expect("cannot generate cert");
        if !output.status.success() {
            panic!("generate cert failed");
        }
    }
    Credential::CertificateFile(CertificateFile::new(
        key_path.display().to_string(),
        cert_path.display().to_string(),
    ))
}

fn buffers_to_string(buffers: &[BufferRef]) -> usize {
    let mut v = Vec::new();
    for b in buffers {
        v.extend_from_slice(b.as_bytes());
    }
    v.len()
}
