use std::{
    ffi::c_void,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    thread::sleep,
    time::Duration,
};

use msquic::{
    Addr, BufferRef, CertificateFile, Configuration, Connection, ConnectionEvent, ConnectionRef,
    ConnectionShutdownFlags, Credential, CredentialConfig, CredentialFlags, Listener,
    ListenerEvent, Registration, RegistrationConfig, SendFlags, ServerResumptionLevel, Settings,
    Status, Stream, StreamEvent, StreamOpenFlags, StreamRef, StreamShutdownFlags, StreamStartFlags,
};

fn main() {
    let my_ip = "94.156.178.64";
    //let cred = get_test_cred();

    let reg = Registration::new(&RegistrationConfig::default()).unwrap();
    let alpn = [BufferRef::from("qtest")];

    // create client and send msg
    let client_settings = Settings::new().set_IdleTimeoutMs(100000);

    let client_config = Configuration::open(&reg, &alpn, Some(&client_settings)).unwrap();
    {
        let cred_config = CredentialConfig::new_client()
            .set_credential_flags(CredentialFlags::NO_CERTIFICATE_VALIDATION);
        client_config.load_credential(&cred_config).unwrap();
    }

    let (c_tx, c_rx) = std::sync::mpsc::channel::<String>();
    {
        let stream_handler = move |stream: StreamRef, ev: StreamEvent| {
            println!("Client stream event: {ev:?}");
            match ev {
                StreamEvent::StartComplete { id, .. } => {
                    // assert_eq!(stream.get_stream_id().unwrap(), id);
                }
                StreamEvent::SendComplete {
                    cancelled,
                    client_context,
                } => {
                    println!("cancelled {}", cancelled);
                    let _ = unsafe {
                        Box::from_raw(client_context as *mut (Vec<u8>, Box<[BufferRef; 1]>))
                    };
                }
                StreamEvent::Receive { buffers, .. } => {
                    // send the result to main thread.
                    let s = buffers_to_string(buffers);
                    c_tx.send(s).unwrap();
                }
                StreamEvent::ShutdownComplete { .. } => {
                    let _ = unsafe { Stream::from_raw(stream.as_raw()) };
                }
                _ => {}
            }
            Ok(())
        };

        let conn_handler = move |conn: ConnectionRef, ev: ConnectionEvent| {
            println!("Client connection event: {ev:?}");
            match ev {
                ConnectionEvent::Connected { .. } => {
                    // open stream and send
                    let f_send = || {
                        let s = Stream::open(
                            &conn,
                            StreamOpenFlags::UNIDIRECTIONAL,
                            stream_handler.clone(),
                        )?;
                        s.start(StreamStartFlags::NONE)?;
                        // BufferRef needs to be heap allocated
                        let b = vec![42u8; 1024 * 1024];
                        let b_ref = Box::new([BufferRef::from((*b).as_ref())]);
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
                    };
                    if f_send().is_err() {
                        println!("Client send failed");
                        // conn.shutdown(ConnectionShutdownFlags::NONE, 0);
                    }
                    println!("Sent");
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

        conn.start(&client_config, my_ip, 4567).unwrap();

        let client_s = c_rx
            .recv_timeout(std::time::Duration::from_secs(3000))
            .expect("Client failed receive response.");

        println!("{}", client_s);
    }
}

fn buffers_to_string(buffers: &[BufferRef]) -> String {
    let mut v = Vec::new();
    for b in buffers {
        v.extend_from_slice(b.as_bytes());
    }
    String::from_utf8_lossy(v.as_slice()).to_string()
}
