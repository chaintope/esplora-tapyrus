pub use tapyrus::{util::address, Block, BlockHeader, OutPoint, Transaction, TxIn, TxOut};

use tapyrus::network::constants::Network as BNetwork;
use tapyrus::network::constants::NetworkId;
use tapyrus::BlockHash;

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, RwLock};

pub type Value = u64;

lazy_static! {
    static ref CACHED_GENESIS: Arc<RwLock<HashMap<Network, BlockHash>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

#[derive(Debug, Copy, Clone, PartialEq, Hash, Serialize, Ord, PartialOrd, Eq)]
pub struct Network {
    pub network_type: NetworkType,
    pub id: u32,
}

impl Network {
    pub fn new(network_type: &str, id: u32) -> Self {
        Network {
            network_type: NetworkType::new(network_type),
            id: id,
        }
    }

    pub fn magic(&self) -> u32 {
        NetworkId::from(self.id).magic()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Hash, Serialize, Ord, PartialOrd, Eq)]
pub enum NetworkType {
    Prod,
    Dev,
}

impl NetworkType {
    pub fn names() -> Vec<String> {
        return vec!["prod".to_string(), "dev".to_string()];
    }
}

impl NetworkType {
    fn new(network_name: &str) -> Self {
        match network_name {
            "prod" => NetworkType::Prod,
            "dev" => NetworkType::Dev,
            _ => panic!("unsupported Tapyrus network: {:?}", network_name),
        }
    }
}

impl From<Network> for BNetwork {
    fn from(network: Network) -> Self {
        match network.network_type {
            NetworkType::Prod => BNetwork::Prod,
            NetworkType::Dev => BNetwork::Dev,
        }
    }
}

impl From<BNetwork> for NetworkType {
    fn from(network: BNetwork) -> Self {
        match network {
            BNetwork::Prod => NetworkType::Prod,
            BNetwork::Dev => NetworkType::Dev,
        }
    }
}
