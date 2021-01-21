pub use tapyrus::{util::address, Block, BlockHeader, OutPoint, Transaction, TxIn, TxOut};

use tapyrus::network::constants::Network as BNetwork;
use tapyrus::BlockHash;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub type Value = u64;

lazy_static! {
    static ref CACHED_GENESIS: Arc<RwLock<HashMap<Network, BlockHash>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

#[derive(Debug, Copy, Clone, PartialEq, Hash, Serialize, Ord, PartialOrd, Eq)]
pub enum Network {
    Prod,
    Dev
}

impl Network {

    pub fn magic(self) -> u32 {
        match self {
            Network::Prod => 0xD9B4_BEF9,
            Network::Dev => 0x0709_110B,
        }
    }

    pub fn names() -> Vec<String> {
        return vec![
            "prod".to_string(),
            "dev".to_string(),
        ];
    }
}

impl From<&str> for Network {
    fn from(network_name: &str) -> Self {
        match network_name {
            "prod" => Network::Prod,
            "dev" => Network::Dev,
            _ => panic!("unsupported Tapyrus network: {:?}", network_name),
        }
    }
}

impl From<Network> for BNetwork {
    fn from(network: Network) -> Self {
        match network {
            Network::Prod => BNetwork::Prod,
            Network::Dev => BNetwork::Dev,
        }
    }
}

impl From<BNetwork> for Network {
    fn from(network: BNetwork) -> Self {
        match network {
            BNetwork::Prod => Network::Prod,
            BNetwork::Dev => Network::Dev,
        }
    }
}
