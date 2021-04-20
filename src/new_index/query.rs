use rayon::prelude::*;

use openassets_tapyrus::openassets::marker_output::TxOutExt;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, RwLock, RwLockReadGuard};
use std::time::{Duration, Instant};

use crate::chain::{Network, NetworkType, OutPoint, Transaction, TxOut};
use crate::config::Config;
use crate::daemon::Daemon;
use crate::errors::*;
use crate::new_index::color::ColoredStats;
use crate::new_index::schema::StatsMap;
use crate::new_index::{ChainQuery, Mempool, SpendingInput, Utxo};
use crate::open_assets::{compute_assets, OpenAsset};
use crate::util::{is_spendable, BlockId, Bytes, TransactionStatus};

use tapyrus::{ColorIdentifier, Txid};

const FEE_ESTIMATES_TTL: u64 = 60; // seconds

const CONF_TARGETS: [u16; 28] = [
    1u16, 2u16, 3u16, 4u16, 5u16, 6u16, 7u16, 8u16, 9u16, 10u16, 11u16, 12u16, 13u16, 14u16, 15u16,
    16u16, 17u16, 18u16, 19u16, 20u16, 21u16, 22u16, 23u16, 24u16, 25u16, 144u16, 504u16, 1008u16,
];

pub struct Query {
    chain: Arc<ChainQuery>, // TODO: should be used as read-only
    mempool: Arc<RwLock<Mempool>>,
    daemon: Arc<Daemon>,
    config: Arc<Config>,
    cached_estimates: RwLock<(HashMap<u16, f64>, Option<Instant>)>,
    cached_relayfee: RwLock<Option<f64>>,
}

impl Query {
    pub fn new(
        chain: Arc<ChainQuery>,
        mempool: Arc<RwLock<Mempool>>,
        daemon: Arc<Daemon>,
        config: Arc<Config>,
    ) -> Self {
        Query {
            chain,
            mempool,
            daemon,
            config,
            cached_estimates: RwLock::new((HashMap::new(), None)),
            cached_relayfee: RwLock::new(None),
        }
    }

    pub fn chain(&self) -> &ChainQuery {
        &self.chain
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn network(&self) -> Network {
        self.config.network
    }

    pub fn mempool(&self) -> RwLockReadGuard<Mempool> {
        self.mempool.read().unwrap()
    }

    pub fn broadcast_raw(&self, txhex: &str) -> Result<Txid> {
        let txid = self.daemon.broadcast_raw(txhex)?;
        self.mempool
            .write()
            .unwrap()
            .add_by_txid(&self.daemon, &txid);
        Ok(txid)
    }

    pub fn utxo(&self, scripthash: &[u8]) -> Result<Vec<Utxo>> {
        let mut utxos = self.chain.utxo(scripthash, self.config.utxos_limit)?;
        let mempool = self.mempool();
        utxos.retain(|utxo| !mempool.has_spend(&OutPoint::from(utxo)));
        utxos.extend(mempool.utxo(scripthash));
        Ok(utxos)
    }

    pub fn history_txids(&self, scripthash: &[u8], limit: usize) -> Vec<(Txid, Option<BlockId>)> {
        let confirmed_txids = self.chain.history_txids(scripthash, limit);
        let confirmed_len = confirmed_txids.len();
        let confirmed_txids = confirmed_txids.into_iter().map(|(tx, b)| (tx, Some(b)));

        let mempool_txids = self
            .mempool()
            .history_txids(scripthash, limit - confirmed_len)
            .into_iter()
            .map(|tx| (tx, None));

        confirmed_txids.chain(mempool_txids).collect()
    }

    pub fn stats(&self, scripthash: &[u8]) -> (StatsMap, StatsMap) {
        (
            self.chain.stats(scripthash),
            self.mempool().stats(scripthash),
        )
    }

    pub fn lookup_txn(&self, txid: &Txid) -> Option<Transaction> {
        self.chain
            .lookup_txn(txid, None)
            .or_else(|| self.mempool().lookup_txn(txid))
    }
    pub fn lookup_raw_txn(&self, txid: &Txid) -> Option<Bytes> {
        self.chain
            .lookup_raw_txn(txid, None)
            .or_else(|| self.mempool().lookup_raw_txn(txid))
    }

    pub fn lookup_txos(&self, outpoints: &BTreeSet<OutPoint>) -> HashMap<OutPoint, TxOut> {
        // the mempool lookup_txos() internally looks up confirmed txos as well
        self.mempool()
            .lookup_txos(outpoints)
            .expect("failed loading txos")
    }

    pub fn lookup_spend(&self, outpoint: &OutPoint) -> Option<SpendingInput> {
        self.chain
            .lookup_spend(outpoint)
            .or_else(|| self.mempool().lookup_spend(outpoint))
    }

    pub fn lookup_tx_spends(&self, tx: Transaction) -> Vec<Option<SpendingInput>> {
        let txid = tx.malfix_txid();

        tx.output
            .par_iter()
            .enumerate()
            .map(|(vout, txout)| {
                if is_spendable(txout) {
                    self.lookup_spend(&OutPoint {
                        txid,
                        vout: vout as u32,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn load_assets(&self, tx: &Transaction) -> Result<Vec<Option<OpenAsset>>> {
        let network = tapyrus::network::constants::Network::from(self.config.network);
        Ok(self.get_open_assets_colored_outputs(network, &tx))
    }

    fn get_open_assets_colored_outputs(
        &self,
        network_type: tapyrus::network::constants::Network,
        txn: &Transaction,
    ) -> Vec<Option<OpenAsset>> {
        if txn.is_coin_base() {
            txn.output.iter().map(|_| None).collect()
        } else {
            for (i, val) in txn.output.iter().enumerate() {
                let payload = val.get_oa_payload();
                if let Ok(marker) = payload {
                    let prev_outs = txn
                        .input
                        .iter()
                        .map(|input| {
                            self.get_output(&input.previous_output.txid, input.previous_output.vout)
                        })
                        .collect();
                    return compute_assets(
                        prev_outs,
                        i,
                        txn,
                        marker.quantities,
                        network_type,
                        &marker.metadata,
                    );
                }
            }
            txn.output.iter().map(|_| None).collect()
        }
    }

    fn get_output(&self, txid: &Txid, index: u32) -> (TxOut, Option<OpenAsset>) {
        let txn = self.lookup_txn(txid).expect("txn not found");
        let colored_outputs = self.load_assets(&txn).expect("asset not found");
        (
            txn.output[index as usize].clone(),
            colored_outputs[index as usize].clone(),
        )
    }

    pub fn get_tx_status(&self, txid: &Txid) -> TransactionStatus {
        TransactionStatus::from(self.chain.tx_confirming_block(txid))
    }

    pub fn get_mempool_tx_fee(&self, txid: &Txid) -> Option<u64> {
        self.mempool().get_tx_fee(txid)
    }

    pub fn has_unconfirmed_parents(&self, txid: &Txid) -> bool {
        self.mempool().has_unconfirmed_parents(txid)
    }

    pub fn estimate_fee(&self, conf_target: u16) -> Option<f64> {
        if self.config.network.network_type == NetworkType::Dev {
            return self.get_relayfee().ok();
        }
        if let (ref cache, Some(cache_time)) = *self.cached_estimates.read().unwrap() {
            if cache_time.elapsed() < Duration::from_secs(FEE_ESTIMATES_TTL) {
                return cache.get(&conf_target).copied();
            }
        }

        self.update_fee_estimates();
        self.cached_estimates
            .read()
            .unwrap()
            .0
            .get(&conf_target)
            .copied()
    }

    pub fn estimate_fee_map(&self) -> HashMap<u16, f64> {
        if let (ref cache, Some(cache_time)) = *self.cached_estimates.read().unwrap() {
            if cache_time.elapsed() < Duration::from_secs(FEE_ESTIMATES_TTL) {
                return cache.clone();
            }
        }

        self.update_fee_estimates();
        self.cached_estimates.read().unwrap().0.clone()
    }

    fn update_fee_estimates(&self) {
        match self.daemon.estimatesmartfee_batch(&CONF_TARGETS) {
            Ok(estimates) => {
                *self.cached_estimates.write().unwrap() = (estimates, Some(Instant::now()));
            }
            Err(err) => {
                warn!("failed estimating feerates: {:?}", err);
            }
        }
    }

    pub fn get_relayfee(&self) -> Result<f64> {
        if let Some(cached) = *self.cached_relayfee.read().unwrap() {
            return Ok(cached);
        }

        let relayfee = self.daemon.get_relayfee()?;
        self.cached_relayfee.write().unwrap().replace(relayfee);
        Ok(relayfee)
    }

    pub fn open_assets(&self, utxos: &Vec<Utxo>) -> Result<HashMap<OutPoint, OpenAsset>> {
        let network = tapyrus::network::constants::Network::from(self.config.network);
        let mut map: HashMap<OutPoint, OpenAsset> = HashMap::new();
        utxos.iter().for_each(|o| match self.lookup_txn(&o.txid) {
            Some(tx) => {
                let outputs = self.get_open_assets_colored_outputs(network, &tx);
                match outputs.get(o.vout as usize) {
                    Some(Some(asset)) => {
                        map.insert(OutPoint::new(o.txid, o.vout), asset.clone());
                    }
                    Some(_) => {}
                    None => {}
                }
            }
            None => {}
        });
        Ok(map)
    }

    pub fn get_colored_stats(&self, color_id: &ColorIdentifier) -> (ColoredStats, ColoredStats) {
        (
            self.chain
                .get_colored_stats(color_id)
                .expect("failed to get colored stats"),
            self.mempool()
                .get_colored_stats(color_id)
                .expect("failed to get colored stats"),
        )
    }

    pub fn get_colored_txs(
        &self,
        color_id: &ColorIdentifier,
        last_seen_txid: Option<&Txid>,
        limit: usize,
    ) -> Vec<(Transaction, Option<BlockId>)> {
        let mut txs = vec![];
        txs.extend(
            self.chain()
                .get_colored_txs(color_id, last_seen_txid, limit),
        );
        txs.extend(self.mempool().get_colored_txs(color_id));
        txs
    }
}
