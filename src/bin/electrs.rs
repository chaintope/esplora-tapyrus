extern crate error_chain;
extern crate tapyrus;
#[macro_use]
extern crate log;

extern crate esplora_tapyrus;

use error_chain::ChainedError;
use std::process;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use esplora_tapyrus::{
    config::Config,
    daemon::Daemon,
    electrum::RPC as ElectrumRPC,
    errors::*,
    metrics::Metrics,
    new_index::{precache, ChainQuery, FetchFrom, Indexer, Mempool, Query, Store},
    rest,
    signal::Waiter,
};

fn fetch_from(config: &Config, store: &Store) -> FetchFrom {
    let mut jsonrpc_import = config.jsonrpc_import;
    if !jsonrpc_import {
        // switch over to jsonrpc after the initial sync is done
        jsonrpc_import = store.done_initial_sync();
    }

    if jsonrpc_import {
        // slower, uses JSONRPC (good for incremental updates)
        FetchFrom::Tapyrusd
    } else {
        // faster, uses blk*.dat files (good for initial indexing)
        FetchFrom::BlkFiles
    }
}

fn run_server(config: Arc<Config>) -> Result<()> {
    let signal = Waiter::start();
    let metrics = Metrics::new(config.monitoring_addr);
    metrics.start();

    let daemon = Arc::new(Daemon::new(
        &config.daemon_dir,
        &config.blocks_dir,
        config.daemon_rpc_addr,
        config.cookie_getter(),
        config.network,
        signal.clone(),
        &metrics,
    )?);
    let store = Arc::new(Store::open(&config.db_path.join("newindex"), &config));
    let mut indexer = Indexer::open(
        Arc::clone(&store),
        fetch_from(&config, &store),
        &config,
        &metrics,
    );
    let mut tip = indexer.update(&daemon)?;

    let chain = Arc::new(ChainQuery::new(
        Arc::clone(&store),
        Arc::clone(&daemon),
        &config,
        &metrics,
    ));

    if let Some(ref precache_file) = config.precache_scripts {
        let precache_scripthashes = precache::scripthashes_from_file(precache_file.to_string())
            .expect("cannot load scripts to precache");
        precache::precache(&chain, precache_scripthashes);
    }

    let mempool = Arc::new(RwLock::new(Mempool::new(
        Arc::clone(&chain),
        &metrics,
        Arc::clone(&config),
    )));
    mempool.write().unwrap().update(&daemon)?;

    let query = Arc::new(Query::new(
        Arc::clone(&chain),
        Arc::clone(&mempool),
        Arc::clone(&daemon),
        Arc::clone(&config),
    ));

    // TODO: configuration for which servers to start
    let rest_server = rest::start(Arc::clone(&config), Arc::clone(&query));
    let electrum_server = ElectrumRPC::start(Arc::clone(&config), Arc::clone(&query), &metrics);

    loop {
        if let Err(err) = signal.wait(Duration::from_secs(5), true) {
            info!("stopping server: {}", err);
            rest_server.stop();
            // the electrum server is stopped when dropped
            break;
        }

        // Index new blocks
        let current_tip = daemon.getbestblockhash()?;
        if current_tip != tip {
            indexer.update(&daemon)?;
            tip = current_tip;
        };

        // Update mempool
        mempool.write().unwrap().update(&daemon)?;

        // Update subscribed clients
        electrum_server.notify();
    }
    info!("server stopped");
    Ok(())
}

fn main() {
    let config = Arc::new(Config::from_args());
    if let Err(e) = run_server(config) {
        error!("server failed: {}", e.display_chain());
        process::exit(1);
    }
}
