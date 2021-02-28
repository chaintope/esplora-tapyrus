use crate::chain::Network;
use crate::electrum::discovery::{DiscoveryManager, Service};

pub fn add_default_servers(discovery: &DiscoveryManager, network: Network) {
    match network {
        Network::Prod => {
        }
        Network::Dev => {
        }

        _ => (),
    }
}
