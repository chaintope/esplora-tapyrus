pub use bitcoin::{util::address, Block, BlockHeader, OutPoint, Transaction, TxIn, TxOut};

use bitcoin::blockdata::constants::genesis_block;
use bitcoin::network::constants::Network as BNetwork;
use bitcoin::util::hash::BitcoinHash;
use bitcoin::BlockHash;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub type Value = u64;

lazy_static! {
    static ref CACHED_GENESIS: Arc<RwLock<HashMap<Network, BlockHash>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

#[derive(Debug, Copy, Clone, PartialEq, Hash, Serialize, Ord, PartialOrd, Eq)]
pub enum Network {
    Bitcoin,
    Testnet,
    Regtest,
}

impl Network {
    pub fn genesis_hash(self) -> BlockHash {
        if let Some(block_hash) = CACHED_GENESIS.read().unwrap().get(&self) {
            return *block_hash;
        }

        let block_hash = genesis_block(BNetwork::from(self)).bitcoin_hash();
        CACHED_GENESIS.write().unwrap().insert(self, block_hash);
        block_hash
    }

    pub fn magic(self) -> u32 {
        match self {
            Network::Bitcoin => 0xD9B4_BEF9,
            Network::Testnet => 0x0709_110B,
            Network::Regtest => 0xDAB5_BFFA,
        }
    }

    pub fn names() -> Vec<String> {
        return vec![
            "mainnet".to_string(),
            "testnet".to_string(),
            "regtest".to_string(),
        ];
    }
}

impl From<&str> for Network {
    fn from(network_name: &str) -> Self {
        match network_name {
            "mainnet" => Network::Bitcoin,
            "testnet" => Network::Testnet,
            "regtest" => Network::Regtest,
            _ => panic!("unsupported Bitcoin network: {:?}", network_name),
        }
    }
}

impl From<Network> for BNetwork {
    fn from(network: Network) -> Self {
        match network {
            Network::Bitcoin => BNetwork::Bitcoin,
            Network::Testnet => BNetwork::Testnet,
            Network::Regtest => BNetwork::Regtest,
        }
    }
}

impl From<BNetwork> for Network {
    fn from(network: BNetwork) -> Self {
        match network {
            BNetwork::Bitcoin => Network::Bitcoin,
            BNetwork::Regtest => Network::Regtest,
            BNetwork::Testnet => Network::Testnet, // @FIXME
        }
    }
}
