use anyhow::Result;
use std::{io, net::SocketAddr, sync::Arc};
use tokio::time::Instant;

use std::net::UdpSocket as StdUdpSocket;
use tokio::net::UdpSocket as TokioUdpSocket;

use crate::{
    MAGIC_NUMBER, config::BLOCK_SIZE, metrics::Metrics, now_ms, quic_config::configure_server,
};

pub async fn run(metrics: Arc<Metrics>, address: String, port: u16) -> Result<()> {
    Ok(())
}
