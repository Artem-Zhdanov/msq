use clap::{Parser, arg};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::path::{Path, PathBuf};

pub const BLOCK_SIZE: usize = 300 * 1024;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub subscriber: Vec<Subscriber>,
    pub publisher: Vec<Peers>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Subscriber {
    pub address: String,
    pub ports: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Peers {
    pub addr_peer: String,
    pub ports: String,
}

#[derive(Parser, Debug, Clone, Serialize)]
pub struct CliArgs {
    #[arg(short, long)]
    pub config: PathBuf,
}

pub fn read_yaml<T: DeserializeOwned>(config_path: impl AsRef<Path>) -> anyhow::Result<T> {
    let config_path = config_path.as_ref();
    let Some(path) = config_path.as_os_str().to_str() else {
        anyhow::bail!("Invalid path {:?}", config_path);
    };
    let expanded = PathBuf::from(shellexpand::tilde(path).into_owned());
    let file = std::fs::File::open(&expanded)?;
    let config = serde_yaml::from_reader(file)?;
    Ok(config)
}
