use arraydeque::{ArrayDeque, Wrapping};
use itertools::Itertools;
use tapyrus::consensus::encode::deserialize;
use tapyrus::Txid;

use tapyrus::blockdata::script::ColorIdentifier;
use tapyrus::consensus::encode::serialize;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::iter::FromIterator;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::chain::{Network, OutPoint, Transaction, TxOut};
use crate::config::Config;
use crate::daemon::Daemon;
use crate::errors::*;
use crate::metrics::{GaugeVec, HistogramOpts, HistogramVec, MetricOpts, Metrics};
use crate::new_index::color::colored_tx_history;
use crate::new_index::color::{ColoredStats, ColoredTxHistoryInfo};
use crate::new_index::schema::{update_colored_stats, update_stats};
use crate::new_index::{
    compute_script_hash, schema::FullHash, ChainQuery, FundingInfo, ScriptStats, SpendingInfo,
    SpendingInput, TxHistoryInfo, Utxo,
};
use crate::util::fees::{make_fee_histogram, TxFeeInfo};
use crate::util::{extract_tx_prevouts, full_hash, has_prevout, is_spendable, BlockId, Bytes};

const RECENT_TXS_SIZE: usize = 10;
const BACKLOG_STATS_TTL: u64 = 10;

pub struct Mempool {
    chain: Arc<ChainQuery>,
    config: Arc<Config>,
    txstore: HashMap<Txid, Transaction>,
    feeinfo: HashMap<Txid, TxFeeInfo>,
    history: HashMap<FullHash, Vec<TxHistoryInfo>>, // ScriptHash -> {history_entries}
    colors: HashMap<ColorIdentifier, Vec<ColoredTxHistoryInfo>>,
    edges: HashMap<OutPoint, (Txid, u32)>, // OutPoint -> (spending_txid, spending_vin)
    recent: ArrayDeque<[TxOverview; RECENT_TXS_SIZE], Wrapping>, // The N most recent txs to enter the mempool
    overviews: HashMap<Txid, TxOverview>,
    backlog_stats: (BacklogStats, Instant),

    // monitoring
    latency: HistogramVec, // mempool requests latency
    delta: HistogramVec,   // # of added/removed txs
    count: GaugeVec,       // current state of the mempool
}

// A simplified transaction view used for the list of most recent transactions
#[derive(Serialize)]
pub struct TxOverview {
    txid: Txid,
    fee: u64,
    vsize: u32,
    time: u32,
    value: u64,
}

// A transaction in mempool
#[derive(Serialize, Deserialize)]
pub struct MempoolTx {
    size: u32,
    fee: f32,
    modifiedfee: f32,
    time: u32,
    height: u32,
    descendantcount: u32,
    descendantsize: u32,
    descendantfees: u64,
    ancestorcount: u32,
    ancestorsize: u32,
    ancestorfees: u64,
    txid: Txid,
}

impl Mempool {
    pub fn new(chain: Arc<ChainQuery>, metrics: &Metrics, config: Arc<Config>) -> Self {
        Mempool {
            chain,
            config,
            txstore: HashMap::new(),
            feeinfo: HashMap::new(),
            history: HashMap::new(),
            colors: HashMap::new(),
            edges: HashMap::new(),
            recent: ArrayDeque::new(),
            overviews: HashMap::new(),
            backlog_stats: (
                BacklogStats::default(),
                Instant::now() - Duration::from_secs(BACKLOG_STATS_TTL),
            ),
            latency: metrics.histogram_vec(
                HistogramOpts::new("mempool_latency", "Mempool requests latency (in seconds)"),
                &["part"],
            ),
            delta: metrics.histogram_vec(
                HistogramOpts::new("mempool_delta", "# of transactions added/removed"),
                &["type"],
            ),
            count: metrics.gauge_vec(
                MetricOpts::new("mempool_count", "# of elements currently at the mempool"),
                &["type"],
            ),
        }
    }

    pub fn network(&self) -> Network {
        self.config.network
    }

    pub fn lookup_txn(&self, txid: &Txid) -> Option<Transaction> {
        self.txstore.get(txid).cloned()
    }

    pub fn lookup_raw_txn(&self, txid: &Txid) -> Option<Bytes> {
        self.txstore.get(txid).map(serialize)
    }

    pub fn lookup_spend(&self, outpoint: &OutPoint) -> Option<SpendingInput> {
        self.edges.get(outpoint).map(|(txid, vin)| SpendingInput {
            txid: *txid,
            vin: *vin,
            confirmed: None,
        })
    }

    pub fn has_spend(&self, outpoint: &OutPoint) -> bool {
        self.edges.contains_key(outpoint)
    }

    pub fn get_tx_fee(&self, txid: &Txid) -> Option<u64> {
        Some(self.feeinfo.get(txid)?.fee)
    }

    pub fn has_unconfirmed_parents(&self, txid: &Txid) -> bool {
        let tx = match self.txstore.get(txid) {
            Some(tx) => tx,
            None => return false,
        };
        tx.input
            .iter()
            .any(|txin| self.txstore.contains_key(&txin.previous_output.txid))
    }

    pub fn history(&self, scripthash: &[u8], limit: usize) -> Vec<Transaction> {
        let _timer = self.latency.with_label_values(&["history"]).start_timer();
        self.history
            .get(scripthash)
            .map_or_else(|| vec![], |entries| self._history(entries, limit))
    }

    fn _history(&self, entries: &[TxHistoryInfo], limit: usize) -> Vec<Transaction> {
        entries
            .iter()
            .map(|e| e.get_txid())
            .unique()
            .take(limit)
            .map(|txid| self.txstore.get(&txid).expect("missing mempool tx"))
            .cloned()
            .collect()
    }

    pub fn history_txids(&self, scripthash: &[u8], limit: usize) -> Vec<Txid> {
        let _timer = self
            .latency
            .with_label_values(&["history_txids"])
            .start_timer();
        match self.history.get(scripthash) {
            None => vec![],
            Some(entries) => entries
                .iter()
                .map(|e| e.get_txid())
                .unique()
                .take(limit)
                .collect(),
        }
    }

    pub fn utxo(&self, scripthash: &[u8]) -> Vec<Utxo> {
        let _timer = self.latency.with_label_values(&["utxo"]).start_timer();
        let entries = match self.history.get(scripthash) {
            None => return vec![],
            Some(entries) => entries,
        };

        entries
            .iter()
            .filter_map(|entry| match entry {
                TxHistoryInfo::Funding(info) => Some(Utxo {
                    txid: deserialize(&info.txid).expect("invalid txid"),
                    vout: info.vout as u32,
                    color_id: info.color_id.clone(),
                    value: info.value,
                    confirmed: None,
                }),
                TxHistoryInfo::Spending(_) => None,
            })
            .filter(|utxo| !self.has_spend(&OutPoint::from(utxo)))
            .collect()
    }

    // @XXX avoid code duplication with ChainQuery::stats()?
    pub fn stats(&self, scripthash: &[u8]) -> HashMap<ColorIdentifier, ScriptStats> {
        let _timer = self.latency.with_label_values(&["stats"]).start_timer();
        let entries = match self.history.get(scripthash) {
            None => return HashMap::new(),
            Some(entries) => entries
                .iter()
                .map(|e| (e.clone(), None))
                .collect::<Vec<(TxHistoryInfo, Option<BlockId>)>>(),
        };

        let (stats, _) = update_stats(HashMap::new(), &entries);
        stats
    }

    pub fn get_colored_stats(&self, color_id: &ColorIdentifier) -> Result<ColoredStats> {
        let _timer = self
            .latency
            .with_label_values(&["get_colored_stats"])
            .start_timer();
        let histories = match self.colors.get(color_id) {
            None => vec![],
            Some(entries) => entries
                .iter()
                .map(|e| (e.clone(), None))
                .collect::<Vec<(ColoredTxHistoryInfo, Option<BlockId>)>>(),
        };
        let (stats, _) = update_colored_stats(ColoredStats::new(color_id), &histories)?;
        Ok(stats)
    }

    pub fn get_colored_txs(
        &self,
        color_id: &ColorIdentifier,
    ) -> Vec<(Transaction, Option<BlockId>)> {
        let _timer = self
            .latency
            .with_label_values(&["get_colored_txs"])
            .start_timer();
        let histories = match self.colors.get(color_id) {
            None => vec![],
            Some(entries) => entries
                .iter()
                .map(|info| {
                    self.txstore
                        .get(&info.get_txid())
                        .expect("missing mempool tx")
                })
                .map(|tx| (tx.clone(), None))
                .collect::<Vec<(Transaction, Option<BlockId>)>>(),
        };
        histories
    }

    // Get all txids in the mempool
    pub fn txids(&self) -> Vec<&Txid> {
        let _timer = self.latency.with_label_values(&["txids"]).start_timer();
        self.txstore.keys().collect()
    }

    // Get an overview of the most recent transactions
    pub fn recent_txs_overview(&self) -> Vec<&TxOverview> {
        // We don't bother ever deleting elements from the recent list.
        // It may contain outdated txs that are no longer in the mempool,
        // until they get pushed out by newer transactions.
        self.recent.iter().collect()
    }

    pub fn txs_overview(&self) -> Vec<&TxOverview> {
        let mut txs: Vec<&TxOverview> = self.overviews.values().collect();
        txs.sort_by(|a, b| b.time.cmp(&a.time));
        txs
    }

    pub fn backlog_stats(&self) -> &BacklogStats {
        &self.backlog_stats.0
    }

    pub fn update(&mut self, daemon: &Daemon) -> Result<()> {
        let _timer = self.latency.with_label_values(&["update"]).start_timer();
        let txs = daemon
            .getmempool()
            .chain_err(|| "failed to update mempool from daemon")?;

        let new_txids: HashSet<Txid> = txs.keys().cloned().collect();
        let old_txids = HashSet::from_iter(self.txstore.keys().cloned());
        let to_remove: HashSet<&Txid> = old_txids.difference(&new_txids).collect();

        // Download and add new transactions from tapyrusd's mempool
        let txids: Vec<&Txid> = new_txids.difference(&old_txids).collect();
        let to_add_tx = match daemon.gettransactions(&txids) {
            Ok(txs) => txs,
            Err(err) => {
                warn!("failed to get transactions {:?}: {}", txids, err); // e.g. new block or RBF
                return Ok(()); // keep the mempool until next update()
            }
        };
        let to_add = to_add_tx
            .iter()
            .map(|tx| {
                let mempooltx = txs
                    .get(&tx.malfix_txid())
                    .expect("failed to get mempool tx");
                (mempooltx.time, tx.clone())
            })
            .collect();
        // Add new transactions
        self.add(to_add);
        // Remove missing transactions
        self.remove(to_remove);

        self.count
            .with_label_values(&["txs"])
            .set(self.txstore.len() as f64);

        // Update cached backlog stats (if expired)
        if self.backlog_stats.1.elapsed() > Duration::from_secs(BACKLOG_STATS_TTL) {
            let _timer = self
                .latency
                .with_label_values(&["update_backlog_stats"])
                .start_timer();
            self.backlog_stats = (BacklogStats::new(&self.feeinfo), Instant::now());
        }

        Ok(())
    }

    pub fn add_by_txid(&mut self, daemon: &Daemon, txid: &Txid) {
        if self.txstore.get(txid).is_none() {
            if let Ok(mempooltx) = daemon.getmempooltx(&txid) {
                if let Ok(txs) = daemon.gettransactions(&[&mempooltx.txid]) {
                    self.add(vec![(mempooltx.time, txs.get(0).unwrap().clone())])
                }
            }
        }
    }

    fn add(&mut self, txs: Vec<(u32, Transaction)>) {
        self.delta
            .with_label_values(&["add"])
            .observe(txs.len() as f64);
        let _timer = self.latency.with_label_values(&["add"]).start_timer();

        let mut txids = vec![];
        // Phase 1: add to txstore
        for (_time, tx) in &txs {
            let txid = tx.malfix_txid();
            txids.push(txid);
            self.txstore.insert(txid, tx.clone());
        }
        // Phase 2: index history and spend edges (can fail if some txos cannot be found)
        let txos = match self.lookup_txos(&self.get_prevouts(&txids)) {
            Ok(txos) => txos,
            Err(err) => {
                warn!("lookup txouts failed: {}", err);
                // TODO: should we remove txids from txstore?
                return;
            }
        };
        for (time, tx) in &txs {
            let txid = tx.malfix_txid();
            let tx = self.txstore.get(&txid).expect("missing mempool tx");
            let txid_bytes = full_hash(&txid[..]);
            let prevouts = extract_tx_prevouts(&tx, &txos, false);

            // Get feeinfo for caching and recent tx overview
            let feeinfo = TxFeeInfo::new(&tx, &prevouts, self.config.network);

            // recent is an ArrayDeque that automatically evicts the oldest elements
            self.recent.push_front(TxOverview {
                txid,
                fee: feeinfo.fee,
                vsize: feeinfo.vsize,
                time: *time,
                value: prevouts.values().map(|prevout| prevout.value).sum(),
            });

            self.overviews.insert(
                txid,
                TxOverview {
                    txid,
                    fee: feeinfo.fee,
                    vsize: feeinfo.vsize,
                    time: *time,
                    value: prevouts.values().map(|prevout| prevout.value).sum(),
                },
            );

            self.feeinfo.insert(txid, feeinfo);

            // An iterator over (ScriptHash, TxHistoryInfo)
            let spending = prevouts.into_iter().flat_map(|(input_index, prevout)| {
                let txi = tx.input.get(input_index as usize).unwrap();
                if let Some((color_id, script)) = prevout.script_pubkey.split_color() {
                    vec![
                        (
                            compute_script_hash(&prevout.script_pubkey),
                            TxHistoryInfo::Spending(SpendingInfo {
                                txid: txid_bytes,
                                vin: input_index as u16,
                                prev_txid: full_hash(&txi.previous_output.txid[..]),
                                prev_vout: txi.previous_output.vout as u16,
                                color_id: color_id.clone(),
                                value: prevout.value,
                            }),
                        ),
                        (
                            compute_script_hash(&script),
                            TxHistoryInfo::Spending(SpendingInfo {
                                txid: txid_bytes,
                                vin: input_index as u16,
                                prev_txid: full_hash(&txi.previous_output.txid[..]),
                                prev_vout: txi.previous_output.vout as u16,
                                color_id: color_id.clone(),
                                value: prevout.value,
                            }),
                        ),
                    ]
                } else {
                    vec![(
                        compute_script_hash(&prevout.script_pubkey),
                        TxHistoryInfo::Spending(SpendingInfo {
                            txid: txid_bytes,
                            vin: input_index as u16,
                            prev_txid: full_hash(&txi.previous_output.txid[..]),
                            prev_vout: txi.previous_output.vout as u16,
                            color_id: ColorIdentifier::default(),
                            value: prevout.value,
                        }),
                    )]
                }
            });

            let config = &self.config;

            // An iterator over (ScriptHash, TxHistoryInfo)
            let funding = tx
                .output
                .iter()
                .enumerate()
                .filter(|(_, txo)| is_spendable(txo) || config.index_unspendables)
                .map(|(index, txo)| {
                    if let Some((color_id, script)) = txo.script_pubkey.split_color() {
                        vec![
                            (
                                compute_script_hash(&txo.script_pubkey),
                                TxHistoryInfo::Funding(FundingInfo {
                                    txid: txid_bytes,
                                    vout: index as u16,
                                    color_id: color_id.clone(),
                                    value: txo.value,
                                    open_asset: None,
                                }),
                            ),
                            (
                                compute_script_hash(&script),
                                TxHistoryInfo::Funding(FundingInfo {
                                    txid: txid_bytes,
                                    vout: index as u16,
                                    color_id: color_id.clone(),
                                    value: txo.value,
                                    open_asset: None,
                                }),
                            ),
                        ]
                    } else {
                        vec![(
                            compute_script_hash(&txo.script_pubkey),
                            TxHistoryInfo::Funding(FundingInfo {
                                txid: txid_bytes,
                                vout: index as u16,
                                color_id: ColorIdentifier::default(),
                                value: txo.value,
                                open_asset: None,
                            }),
                        )]
                    }
                })
                .flatten();

            // Index funding/spending history entries and spend edges
            for (scripthash, entry) in funding.chain(spending) {
                self.history
                    .entry(scripthash)
                    .or_insert_with(Vec::new)
                    .push(entry);
            }
            for (i, txi) in tx.input.iter().enumerate() {
                self.edges.insert(txi.previous_output, (txid, i as u32));
            }

            for (color_id, entry) in colored_tx_history(&tx, &txos) {
                self.colors
                    .entry(color_id)
                    .or_insert_with(Vec::new)
                    .push(entry);
            }
        }
    }

    pub fn lookup_txo(&self, outpoint: &OutPoint) -> Result<TxOut> {
        let mut outpoints = BTreeSet::new();
        outpoints.insert(*outpoint);
        Ok(self.lookup_txos(&outpoints)?.remove(outpoint).unwrap())
    }

    pub fn lookup_txos(&self, outpoints: &BTreeSet<OutPoint>) -> Result<HashMap<OutPoint, TxOut>> {
        let _timer = self
            .latency
            .with_label_values(&["lookup_txos"])
            .start_timer();

        let confirmed_txos = self.chain.lookup_avail_txos(outpoints);

        let mempool_txos = outpoints
            .iter()
            .filter(|outpoint| !confirmed_txos.contains_key(outpoint))
            .map(|outpoint| {
                self.txstore
                    .get(&outpoint.txid)
                    .and_then(|tx| tx.output.get(outpoint.vout as usize).cloned())
                    .map(|txout| (*outpoint, txout))
                    .chain_err(|| format!("missing outpoint {:?}", outpoint))
            })
            .collect::<Result<HashMap<OutPoint, TxOut>>>()?;

        let mut txos = confirmed_txos;
        txos.extend(mempool_txos);
        Ok(txos)
    }

    fn get_prevouts(&self, txids: &[Txid]) -> BTreeSet<OutPoint> {
        let _timer = self
            .latency
            .with_label_values(&["get_prevouts"])
            .start_timer();

        txids
            .iter()
            .map(|txid| self.txstore.get(txid).expect("missing mempool tx"))
            .flat_map(|tx| {
                tx.input
                    .iter()
                    .filter(|txin| has_prevout(txin))
                    .map(|txin| txin.previous_output)
            })
            .collect()
    }

    fn remove(&mut self, to_remove: HashSet<&Txid>) {
        self.delta
            .with_label_values(&["remove"])
            .observe(to_remove.len() as f64);
        let _timer = self.latency.with_label_values(&["remove"]).start_timer();

        for txid in &to_remove {
            self.txstore
                .remove(*txid)
                .unwrap_or_else(|| panic!("missing mempool tx {}", txid));

            self.overviews.remove(*txid).or_else(|| {
                warn!("missing mempool tx overviews {}", txid);
                None
            });

            self.feeinfo.remove(*txid).or_else(|| {
                warn!("missing mempool tx feeinfo {}", txid);
                None
            });
        }

        // TODO: make it more efficient (currently it takes O(|mempool|) time)
        self.history.retain(|_scripthash, entries| {
            entries.retain(|entry| !to_remove.contains(&entry.get_txid()));
            !entries.is_empty()
        });

        self.edges
            .retain(|_outpoint, (txid, _vin)| !to_remove.contains(txid));

        self.colors.retain(|_color_id, entries| {
            entries.retain(|entry| !to_remove.contains(&entry.get_txid()));
            !entries.is_empty()
        });
    }
}

#[derive(Serialize)]
pub struct BacklogStats {
    pub count: u32,
    pub vsize: u32,     // in virtual bytes (= weight/4)
    pub total_fee: u64, // in satoshis
    pub fee_histogram: Vec<(f32, u32)>,
}

impl BacklogStats {
    fn default() -> Self {
        BacklogStats {
            count: 0,
            vsize: 0,
            total_fee: 0,
            fee_histogram: vec![(0.0, 0)],
        }
    }

    fn new(feeinfo: &HashMap<Txid, TxFeeInfo>) -> Self {
        let (count, vsize, total_fee) = feeinfo
            .values()
            .fold((0, 0, 0), |(count, vsize, fee), feeinfo| {
                (count + 1, vsize + feeinfo.vsize, fee + feeinfo.fee)
            });

        BacklogStats {
            count,
            vsize,
            total_fee,
            fee_histogram: make_fee_histogram(feeinfo.values().collect()),
        }
    }
}
