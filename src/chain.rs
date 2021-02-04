pub use tapyrus::{util::address, Block, BlockHeader, OutPoint, Transaction, TxIn, TxOut};

use tapyrus::network::constants::Network as BNetwork;
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
pub enum Network {
    Prod,
    Dev,
}

impl Network {
    pub fn names() -> Vec<String> {
        return vec!["prod".to_string(), "dev".to_string()];
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

/// Network ID is identifier of the Tapyrus network
#[derive(Clone, Debug, PartialEq)]
pub struct NetworkId(u32);

impl NetworkId {
    /// Return the network magic bytes, which should be encoded little-endian
    /// at the start of every message
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tapyrus::network::constants::NetworkId;
    ///
    /// let network = NetworkId::from(1);
    /// assert_eq!(network.magic(), 0x00F0FF01);
    /// ```
    pub fn magic(self) -> u32 {
        (33550335 + self.0).swap_bytes()
    }
}

impl From<u32> for NetworkId {
    fn from(n: u32) -> Self {
        NetworkId(n)
    }
}

impl FromStr for NetworkId {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(NetworkId::from(s.parse::<u32>()?))
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for NetworkId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u32(self.0)
    }
}

#[cfg(feature = "serde")]
impl<'de> ::serde::Deserialize<'de> for NetworkId {
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<NetworkId, D::Error> {
        struct NetworkIdVisitor;

        impl<'de> ::serde::de::Visitor<'de> for NetworkIdVisitor {
            type Value = NetworkId;

            fn expecting(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                formatter.write_str("an integer u32")
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: ::serde::de::Error,
            {
                Ok(NetworkId::from(v as u32))
            }
        }

        d.deserialize_u32(NetworkIdVisitor)
    }
}
