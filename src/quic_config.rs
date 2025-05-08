use anyhow::Result;

const MAX_PACKET_SIZE: usize = 1452;
const MAX_DATAGRAM_SIZE: usize = 1452;
const IDLE_TIMEOUT_MS: u64 = 30000;
const INITIAL_MAX_DATA: u64 = 100_000_000; // 100 MB
const INITIAL_MAX_STREAM_DATA: u64 = 10_000_000; // 10 MB

pub fn configure_server() -> Result<()> {
    create_config(true)
}

pub fn configure_client() -> Result<()> {
    create_config(false)
}

pub fn create_config(is_server: bool) -> anyhow::Result<()> {
    todo!()
}
