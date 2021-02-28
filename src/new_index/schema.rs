use bincode::config::Options;
use crypto::digest::Digest;
use crypto::sha2::Sha256;
use itertools::Itertools;
use rayon::prelude::*;
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, RwLock};
use tapyrus::blockdata::script::{ColorIdentifier, Script};
use tapyrus::consensus::encode::{deserialize, serialize};
use tapyrus::hashes::sha256d::Hash as Sha256dHash;
use tapyrus::util::merkleblock::MerkleBlock;
use tapyrus::{BlockHash, Txid, VarInt};

use crate::chain::{BlockHeader, Network, OutPoint, Transaction, TxOut, Value};
use crate::config::Config;
use crate::daemon::Daemon;
use crate::errors::*;
use crate::metrics::{HistogramOpts, HistogramTimer, HistogramVec, Metrics};
use crate::open_assets::OpenAsset;
use crate::util::{
    full_hash, has_prevout, is_spendable, script_to_address, BlockHeaderMeta, BlockId, BlockMeta,
    BlockStatus, Bytes, HeaderEntry, HeaderList,
};

use crate::new_index::db::{DBFlush, DBRow, ReverseScanIterator, ScanIterator, DB};
use crate::new_index::fetch::{start_fetcher, BlockEntry, FetchFrom};

const MIN_HISTORY_ITEMS_TO_CACHE: usize = 100;

pub struct Store {
    // TODO: should be column families
    txstore_db: DB,
    history_db: DB,
    cache_db: DB,
    added_blockhashes: RwLock<HashSet<BlockHash>>,
    indexed_blockhashes: RwLock<HashSet<BlockHash>>,
    indexed_headers: RwLock<HeaderList>,
}

impl Store {
    pub fn open(path: &Path, config: &Config) -> Self {
        let txstore_db = DB::open(&path.join("txstore"), config);
        let added_blockhashes = load_blockhashes(&txstore_db, &BlockRow::done_filter());
        debug!("{} blocks were added", added_blockhashes.len());

        let history_db = DB::open(&path.join("history"), config);
        let indexed_blockhashes = load_blockhashes(&history_db, &BlockRow::done_filter());
        debug!("{} blocks were indexed", indexed_blockhashes.len());

        let cache_db = DB::open(&path.join("cache"), config);

        let headers = if let Some(tip_hash) = txstore_db.get(b"t") {
            let tip_hash = deserialize(&tip_hash).expect("invalid chain tip in `t`");
            let headers_map = load_blockheaders(&txstore_db);
            debug!(
                "{} headers were loaded, tip at {:?}",
                headers_map.len(),
                tip_hash
            );
            HeaderList::new(headers_map, tip_hash)
        } else {
            HeaderList::empty()
        };

        Store {
            txstore_db,
            history_db,
            cache_db,
            added_blockhashes: RwLock::new(added_blockhashes),
            indexed_blockhashes: RwLock::new(indexed_blockhashes),
            indexed_headers: RwLock::new(headers),
        }
    }

    pub fn txstore_db(&self) -> &DB {
        &self.txstore_db
    }

    pub fn history_db(&self) -> &DB {
        &self.history_db
    }

    pub fn cache_db(&self) -> &DB {
        &self.cache_db
    }

    pub fn done_initial_sync(&self) -> bool {
        self.txstore_db.get(b"t").is_some()
    }
}

type UtxoMap = HashMap<OutPoint, (BlockId, ColorIdentifier, Value)>;

#[derive(Debug)]
pub struct Utxo {
    pub txid: Txid,
    pub vout: u32,
    pub confirmed: Option<BlockId>,
    pub color_id: ColorIdentifier,
    pub value: Value,
}

impl From<&Utxo> for OutPoint {
    fn from(utxo: &Utxo) -> Self {
        OutPoint {
            txid: utxo.txid,
            vout: utxo.vout,
        }
    }
}

#[derive(Debug)]
pub struct SpendingInput {
    pub txid: Txid,
    pub vin: u32,
    pub confirmed: Option<BlockId>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ScriptStats {
    pub tx_count: usize,
    pub funded_txo_count: usize,
    pub spent_txo_count: usize,
    pub funded_txo_sum: u64,
    pub spent_txo_sum: u64,
}

impl ScriptStats {
    pub fn default() -> Self {
        ScriptStats {
            tx_count: 0,
            funded_txo_count: 0,
            spent_txo_count: 0,
            funded_txo_sum: 0,
            spent_txo_sum: 0,
        }
    }
}

pub type StatsMap = HashMap<ColorIdentifier, ScriptStats>;

pub struct Indexer {
    store: Arc<Store>,
    flush: DBFlush,
    from: FetchFrom,
    iconfig: IndexerConfig,
    duration: HistogramVec,
}

struct IndexerConfig {
    light_mode: bool,
    address_search: bool,
    index_unspendables: bool,
    network: Network,
}

impl From<&Config> for IndexerConfig {
    fn from(config: &Config) -> Self {
        IndexerConfig {
            light_mode: config.light_mode,
            address_search: config.address_search,
            index_unspendables: config.index_unspendables,
            network: config.network,
        }
    }
}

pub struct ChainQuery {
    store: Arc<Store>, // TODO: should be used as read-only
    daemon: Arc<Daemon>,
    light_mode: bool,
    duration: HistogramVec,
    network: Network,
}

// TODO: &[Block] should be an iterator / a queue.
impl Indexer {
    pub fn open(store: Arc<Store>, from: FetchFrom, config: &Config, metrics: &Metrics) -> Self {
        Indexer {
            store,
            flush: DBFlush::Disable,
            from,
            iconfig: IndexerConfig::from(config),
            duration: metrics.histogram_vec(
                HistogramOpts::new("index_duration", "Index update duration (in seconds)"),
                &["step"],
            ),
        }
    }

    fn start_timer(&self, name: &str) -> HistogramTimer {
        self.duration.with_label_values(&[name]).start_timer()
    }

    fn headers_to_add(&self, new_headers: &[HeaderEntry]) -> Vec<HeaderEntry> {
        let added_blockhashes = self.store.added_blockhashes.read().unwrap();
        new_headers
            .iter()
            .filter(|e| !added_blockhashes.contains(e.hash()))
            .cloned()
            .collect()
    }

    fn headers_to_index(&self, new_headers: &[HeaderEntry]) -> Vec<HeaderEntry> {
        let indexed_blockhashes = self.store.indexed_blockhashes.read().unwrap();
        new_headers
            .iter()
            .filter(|e| !indexed_blockhashes.contains(e.hash()))
            .cloned()
            .collect()
    }

    fn start_auto_compactions(&self, db: &DB) {
        let key = b"F".to_vec();
        if db.get(&key).is_none() {
            db.full_compaction();
            db.put_sync(&key, b"");
            assert!(db.get(&key).is_some());
        }
        db.enable_auto_compaction();
    }

    fn get_new_headers(&self, daemon: &Daemon, tip: &BlockHash) -> Result<Vec<HeaderEntry>> {
        let headers = self.store.indexed_headers.read().unwrap();
        let new_headers = daemon.get_new_headers(&headers, &tip)?;
        let result = headers.order(new_headers);

        if let Some(tip) = result.last() {
            info!("{:?} ({} left to index)", tip, result.len());
        };
        Ok(result)
    }

    pub fn update(&mut self, daemon: &Daemon) -> Result<BlockHash> {
        let daemon = daemon.reconnect()?;
        let tip = daemon.getbestblockhash()?;
        let new_headers = self.get_new_headers(&daemon, &tip)?;

        let to_add = self.headers_to_add(&new_headers);
        debug!(
            "adding transactions from {} blocks using {:?}",
            to_add.len(),
            self.from
        );
        start_fetcher(self.from, &daemon, to_add)?.map(|blocks| self.add(&blocks));
        self.start_auto_compactions(&self.store.txstore_db);

        let to_index = self.headers_to_index(&new_headers);
        debug!(
            "indexing history from {} blocks using {:?}",
            to_index.len(),
            self.from
        );
        start_fetcher(self.from, &daemon, to_index)?.map(|blocks| self.index(&blocks));
        self.start_auto_compactions(&self.store.history_db);

        if let DBFlush::Disable = self.flush {
            debug!("flushing to disk");
            self.store.txstore_db.flush();
            self.store.history_db.flush();
            self.flush = DBFlush::Enable;
        }

        // update the synced tip *after* the new data is flushed to disk
        debug!("updating synced tip to {:?}", tip);
        self.store.txstore_db.put_sync(b"t", &serialize(&tip));

        let mut headers = self.store.indexed_headers.write().unwrap();
        headers.apply(new_headers);
        assert_eq!(tip, *headers.tip());

        if let FetchFrom::BlkFiles = self.from {
            self.from = FetchFrom::Tapyrusd;
        }

        Ok(tip)
    }

    fn add(&self, blocks: &[BlockEntry]) {
        // TODO: skip orphaned blocks?
        let rows = {
            let _timer = self.start_timer("add_process");
            add_blocks(blocks, &self.iconfig)
        };
        {
            let _timer = self.start_timer("add_write");
            self.store.txstore_db.write(rows, self.flush);
        }

        self.store
            .added_blockhashes
            .write()
            .unwrap()
            .extend(blocks.iter().map(|b| b.entry.hash()));
    }

    fn index(&self, blocks: &[BlockEntry]) {
        let previous_txos_map = {
            let _timer = self.start_timer("index_lookup");
            lookup_txos(&self.store.txstore_db, &get_previous_txos(blocks), false)
        };
        let rows = {
            let _timer = self.start_timer("index_process");
            let added_blockhashes = self.store.added_blockhashes.read().unwrap();
            for b in blocks {
                let blockhash = b.entry.hash();
                // TODO: replace by lookup into txstore_db?
                if !added_blockhashes.contains(blockhash) {
                    panic!("cannot index block {} (missing from store)", blockhash);
                }
            }
            index_blocks(blocks, &previous_txos_map, &self.iconfig)
        };
        self.store.history_db.write(rows, self.flush);
    }
}

impl ChainQuery {
    pub fn new(store: Arc<Store>, daemon: Arc<Daemon>, config: &Config, metrics: &Metrics) -> Self {
        ChainQuery {
            store,
            daemon,
            light_mode: config.light_mode,
            network: config.network,
            duration: metrics.histogram_vec(
                HistogramOpts::new("query_duration", "Index query duration (in seconds)"),
                &["name"],
            ),
        }
    }

    pub fn network(&self) -> Network {
        self.network
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    fn start_timer(&self, name: &str) -> HistogramTimer {
        self.duration.with_label_values(&[name]).start_timer()
    }

    pub fn get_block_txids(&self, hash: &BlockHash) -> Option<Vec<Txid>> {
        let _timer = self.start_timer("get_block_txids");

        if self.light_mode {
            // TODO fetch block as binary from REST API instead of as hex
            let mut blockinfo = self.daemon.getblock_raw(hash, 1).ok()?;
            Some(serde_json::from_value(blockinfo["tx"].take()).unwrap())
        } else {
            self.store
                .txstore_db
                .get(&BlockRow::txids_key(full_hash(&hash[..])))
                .map(|val| bincode::deserialize(&val).expect("failed to parse block txids"))
        }
    }

    pub fn get_block_meta(&self, hash: &BlockHash) -> Option<BlockMeta> {
        let _timer = self.start_timer("get_block_meta");

        if self.light_mode {
            let blockinfo = self.daemon.getblock_raw(hash, 1).ok()?;
            Some(serde_json::from_value(blockinfo).unwrap())
        } else {
            self.store
                .txstore_db
                .get(&BlockRow::meta_key(full_hash(&hash[..])))
                .map(|val| bincode::deserialize(&val).expect("failed to parse BlockMeta"))
        }
    }

    pub fn get_block_raw(&self, hash: &BlockHash) -> Option<Vec<u8>> {
        let _timer = self.start_timer("get_block_raw");

        if self.light_mode {
            let blockhex = self.daemon.getblock_raw(hash, 0).ok()?;
            Some(hex::decode(blockhex.as_str().unwrap()).unwrap())
        } else {
            let entry = self.header_by_hash(hash)?;
            let meta = self.get_block_meta(hash)?;
            let txids = self.get_block_txids(hash)?;

            // Reconstruct the raw block using the header and txids,
            // as <raw header><tx count varint><raw txs>
            let mut raw = Vec::with_capacity(meta.size as usize);

            raw.append(&mut serialize(entry.header()));
            raw.append(&mut serialize(&VarInt(txids.len() as u64)));

            for txid in txids {
                // we don't need to provide the blockhash because we know we're not in light mode
                raw.append(&mut self.lookup_raw_txn(&txid, None)?);
            }

            Some(raw)
        }
    }

    pub fn get_block_header(&self, hash: &BlockHash) -> Option<BlockHeader> {
        let _timer = self.start_timer("get_block_header");
        Some(self.header_by_hash(hash)?.header().clone())
    }

    pub fn get_mtp(&self, height: usize) -> u32 {
        let _timer = self.start_timer("get_block_mtp");
        self.store.indexed_headers.read().unwrap().get_mtp(height)
    }

    pub fn get_block_with_meta(&self, hash: &BlockHash) -> Option<BlockHeaderMeta> {
        let _timer = self.start_timer("get_block_with_meta");
        let header_entry = self.header_by_hash(hash)?;
        Some(BlockHeaderMeta {
            meta: self.get_block_meta(hash)?,
            mtp: self.get_mtp(header_entry.height()),
            header_entry,
        })
    }

    pub fn history_iter_scan(&self, code: u8, hash: &[u8], start_height: usize) -> ScanIterator {
        self.store.history_db.iter_scan_from(
            &TxHistoryRow::filter(code, &hash[..]),
            &TxHistoryRow::prefix_height(code, &hash[..], start_height as u32),
        )
    }
    fn history_iter_scan_reverse(&self, code: u8, hash: &[u8]) -> ReverseScanIterator {
        self.store.history_db.iter_scan_reverse(
            &TxHistoryRow::filter(code, &hash[..]),
            &TxHistoryRow::prefix_end(code, &hash[..]),
        )
    }

    pub fn history(
        &self,
        scripthash: &[u8],
        last_seen_txid: Option<&Txid>,
        limit: usize,
    ) -> Vec<(Transaction, BlockId)> {
        // scripthash lookup
        self._history(b'H', scripthash, last_seen_txid, limit)
    }

    fn _history(
        &self,
        code: u8,
        hash: &[u8],
        last_seen_txid: Option<&Txid>,
        limit: usize,
    ) -> Vec<(Transaction, BlockId)> {
        let _timer_scan = self.start_timer("history");
        let txs_conf = self
            .history_iter_scan_reverse(code, hash)
            .map(|row| TxHistoryRow::from_row(row).get_txid())
            // XXX: unique() requires keeping an in-memory list of all txids, can we avoid that?
            .unique()
            // TODO seek directly to last seen tx without reading earlier rows
            .skip_while(|txid| {
                // skip until we reach the last_seen_txid
                last_seen_txid.map_or(false, |last_seen_txid| last_seen_txid != txid)
            })
            .skip(match last_seen_txid {
                Some(_) => 1, // skip the last_seen_txid itself
                None => 0,
            })
            .filter_map(|txid| self.tx_confirming_block(&txid).map(|b| (txid, b)))
            .take(limit)
            .collect::<Vec<(Txid, BlockId)>>();

        self.lookup_txns(&txs_conf)
            .expect("failed looking up txs in history index")
            .into_iter()
            .zip(txs_conf)
            .map(|(tx, (_, blockid))| (tx, blockid))
            .collect()
    }

    pub fn history_txids(&self, scripthash: &[u8], limit: usize) -> Vec<(Txid, BlockId)> {
        // scripthash lookup
        self._history_txids(b'H', scripthash, limit)
    }

    fn _history_txids(&self, code: u8, hash: &[u8], limit: usize) -> Vec<(Txid, BlockId)> {
        let _timer = self.start_timer("history_txids");
        self.history_iter_scan(code, hash, 0)
            .map(|row| TxHistoryRow::from_row(row).get_txid())
            .unique()
            .filter_map(|txid| self.tx_confirming_block(&txid).map(|b| (txid, b)))
            .take(limit)
            .collect()
    }

    // TODO: avoid duplication with stats/stats_delta?
    pub fn utxo(&self, scripthash: &[u8], limit: usize) -> Result<Vec<Utxo>> {
        let _timer = self.start_timer("utxo");

        // get the last known utxo set and the blockhash it was updated for.
        // invalidates the cache if the block was orphaned.
        let cache: Option<(UtxoMap, usize)> = self
            .store
            .cache_db
            .get(&UtxoCacheRow::key(scripthash))
            .map(|c| bincode::deserialize(&c).unwrap())
            .and_then(|(utxos_cache, blockhash)| {
                self.height_by_hash(&blockhash)
                    .map(|height| (utxos_cache, height))
            })
            .map(|(utxos_cache, height)| (from_utxo_cache(utxos_cache, self), height));
        let had_cache = cache.is_some();

        // update utxo set with new transactions since
        let (newutxos, lastblock, processed_items) = cache.map_or_else(
            || self.utxo_delta(scripthash, HashMap::new(), 0, limit),
            |(oldutxos, blockheight)| self.utxo_delta(scripthash, oldutxos, blockheight + 1, limit),
        )?;

        // save updated utxo set to cache
        if let Some(lastblock) = lastblock {
            if had_cache || processed_items > MIN_HISTORY_ITEMS_TO_CACHE {
                self.store.cache_db.write(
                    vec![UtxoCacheRow::new(scripthash, &newutxos, &lastblock).into_row()],
                    DBFlush::Enable,
                );
            }
        }

        // format as Utxo objects
        Ok(newutxos
            .into_iter()
            .map(|(outpoint, (blockid, color_id, value))| Utxo {
                txid: outpoint.txid,
                vout: outpoint.vout,
                color_id,
                value,
                confirmed: Some(blockid),
            })
            .collect())
    }

    fn utxo_delta(
        &self,
        scripthash: &[u8],
        init_utxos: UtxoMap,
        start_height: usize,
        limit: usize,
    ) -> Result<(UtxoMap, Option<BlockHash>, usize)> {
        let _timer = self.start_timer("utxo_delta");
        let history_iter = self
            .history_iter_scan(b'H', scripthash, start_height)
            .map(TxHistoryRow::from_row)
            .filter_map(|history| {
                self.tx_confirming_block(&history.get_txid())
                    .map(|b| (history, b))
            });

        let mut utxos = init_utxos;
        let mut processed_items = 0;
        let mut lastblock = None;

        for (history, blockid) in history_iter {
            processed_items += 1;
            lastblock = Some(blockid.hash);

            match history.key.txinfo {
                TxHistoryInfo::Funding(ref info) => utxos.insert(
                    history.get_funded_outpoint(),
                    (blockid, info.color_id.clone(), info.value),
                ),
                TxHistoryInfo::Spending(_) => utxos.remove(&history.get_funded_outpoint()),
            };

            // abort if the utxo set size excedees the limit at any point in time
            if utxos.len() > limit {
                bail!(ErrorKind::TooPopular)
            }
        }

        Ok((utxos, lastblock, processed_items))
    }

    pub fn stats_iter_scan(
        &self,
        scripthash: &[u8],
        start_color_id: ColorIdentifier,
    ) -> ScanIterator {
        self.store.cache_db.iter_scan_from(
            &StatsCacheRow::key(scripthash),
            &StatsCacheRow::prefix_color_id(scripthash, start_color_id),
        )
    }

    pub fn stats(&self, scripthash: &[u8]) -> StatsMap {
        let _timer = self.start_timer("stats");

        let mut blockheight = None;
        let stats: StatsMap = self
            .stats_iter_scan(scripthash, ColorIdentifier::default())
            .map(StatsCacheRow::from_row)
            .map(|s| {
                let color_id = s.key.color_id;
                let (stat, blockhash): (ScriptStats, BlockHash) =
                    bincode::deserialize(&s.value).unwrap();
                blockheight = self.height_by_hash(&blockhash);
                (color_id, stat)
            })
            .collect();

        let (newstats, lastblock) = match blockheight {
            Some(height) => self.stats_delta(scripthash, stats, height + 1),
            None => self.stats_delta(scripthash, stats, 0),
        };

        // save updated stats to cache
        if let Some(lastblock) = lastblock {
            if self.txo_count(&newstats) > MIN_HISTORY_ITEMS_TO_CACHE {
                for (key, stat) in &newstats {
                    self.store.cache_db.write(
                        vec![
                            StatsCacheRow::new(scripthash, key.clone(), &stat, &lastblock)
                                .into_row(),
                        ],
                        DBFlush::Enable,
                    );
                }
            }
        }

        newstats
    }

    fn txo_count(&self, stats: &StatsMap) -> usize {
        stats
            .values()
            .fold(0, |sum, x| sum + x.funded_txo_count + x.spent_txo_count)
    }

    fn stats_delta(
        &self,
        scripthash: &[u8],
        init_stats: StatsMap,
        start_height: usize,
    ) -> (StatsMap, Option<BlockHash>) {
        let _timer = self.start_timer("stats_delta"); // TODO: measure also the number of txns processed.
        let histories = self
            .history_iter_scan(b'H', scripthash, start_height)
            .map(TxHistoryRow::from_row)
            .filter_map(|history| {
                self.tx_confirming_block(&history.get_txid())
                    .map(|blockid| (history.key.txinfo, Some(blockid)))
            })
            .collect();

        update_stats(init_stats, &histories)
    }

    pub fn address_search(&self, prefix: &str, limit: usize) -> Vec<String> {
        let _timer_scan = self.start_timer("address_search");
        self.store
            .history_db
            .iter_scan(&addr_search_filter(prefix))
            .take(limit)
            .map(|row| std::str::from_utf8(&row.key[1..]).unwrap().to_string())
            .collect()
    }

    fn header_by_hash(&self, hash: &BlockHash) -> Option<HeaderEntry> {
        self.store
            .indexed_headers
            .read()
            .unwrap()
            .header_by_blockhash(hash)
            .cloned()
    }

    // Get the height of a blockhash, only if its part of the best chain
    pub fn height_by_hash(&self, hash: &BlockHash) -> Option<usize> {
        self.store
            .indexed_headers
            .read()
            .unwrap()
            .header_by_blockhash(hash)
            .map(|header| header.height())
    }

    pub fn header_by_height(&self, height: usize) -> Option<HeaderEntry> {
        self.store
            .indexed_headers
            .read()
            .unwrap()
            .header_by_height(height)
            .cloned()
    }

    pub fn hash_by_height(&self, height: usize) -> Option<BlockHash> {
        self.store
            .indexed_headers
            .read()
            .unwrap()
            .header_by_height(height)
            .map(|entry| *entry.hash())
    }

    pub fn blockid_by_height(&self, height: usize) -> Option<BlockId> {
        self.store
            .indexed_headers
            .read()
            .unwrap()
            .header_by_height(height)
            .map(BlockId::from)
    }

    // returns None for orphaned blocks
    pub fn blockid_by_hash(&self, hash: &BlockHash) -> Option<BlockId> {
        self.store
            .indexed_headers
            .read()
            .unwrap()
            .header_by_blockhash(hash)
            .map(BlockId::from)
    }

    pub fn best_height(&self) -> usize {
        self.store.indexed_headers.read().unwrap().len() - 1
    }

    pub fn best_hash(&self) -> BlockHash {
        *self.store.indexed_headers.read().unwrap().tip()
    }

    pub fn best_header(&self) -> HeaderEntry {
        let headers = self.store.indexed_headers.read().unwrap();
        headers
            .header_by_blockhash(headers.tip())
            .expect("missing chain tip")
            .clone()
    }

    // TODO: can we pass txids as a "generic iterable"?
    // TODO: should also use a custom ThreadPoolBuilder?
    pub fn lookup_txns(&self, txids: &[(Txid, BlockId)]) -> Result<Vec<Transaction>> {
        let _timer = self.start_timer("lookup_txns");
        txids
            .par_iter()
            .map(|(txid, blockid)| {
                self.lookup_txn(txid, Some(&blockid.hash))
                    .chain_err(|| "missing tx")
            })
            .collect::<Result<Vec<Transaction>>>()
    }

    pub fn lookup_txn(&self, txid: &Txid, blockhash: Option<&BlockHash>) -> Option<Transaction> {
        let _timer = self.start_timer("lookup_txn");
        self.lookup_raw_txn(txid, blockhash).map(|rawtx| {
            let txn: Transaction = deserialize(&rawtx).expect("failed to parse Transaction");
            assert_eq!(*txid, txn.malfix_txid());
            txn
        })
    }

    pub fn lookup_raw_txn(&self, txid: &Txid, blockhash: Option<&BlockHash>) -> Option<Bytes> {
        let _timer = self.start_timer("lookup_raw_txn");

        if self.light_mode {
            let queried_blockhash =
                blockhash.map_or_else(|| self.tx_confirming_block(txid).map(|b| b.hash), |_| None);
            let blockhash = blockhash.or_else(|| queried_blockhash.as_ref())?;
            // TODO fetch transaction as binary from REST API instead of as hex
            let txhex = self
                .daemon
                .gettransaction_raw(txid, blockhash, false)
                .ok()?;
            Some(hex::decode(txhex.as_str().unwrap()).unwrap())
        } else {
            self.store.txstore_db.get(&TxRow::key(&txid[..]))
        }
    }

    pub fn lookup_txo(&self, outpoint: &OutPoint) -> Option<TxOut> {
        let _timer = self.start_timer("lookup_txo");
        lookup_txo(&self.store.txstore_db, outpoint)
    }

    pub fn lookup_txos(&self, outpoints: &BTreeSet<OutPoint>) -> HashMap<OutPoint, TxOut> {
        let _timer = self.start_timer("lookup_txos");
        lookup_txos(&self.store.txstore_db, outpoints, false)
    }

    pub fn lookup_avail_txos(&self, outpoints: &BTreeSet<OutPoint>) -> HashMap<OutPoint, TxOut> {
        let _timer = self.start_timer("lookup_available_txos");
        lookup_txos(&self.store.txstore_db, outpoints, true)
    }

    pub fn lookup_spend(&self, outpoint: &OutPoint) -> Option<SpendingInput> {
        let _timer = self.start_timer("lookup_spend");
        self.store
            .history_db
            .iter_scan(&TxEdgeRow::filter(&outpoint))
            .map(TxEdgeRow::from_row)
            .find_map(|edge| {
                let txid: Txid = deserialize(&edge.key.spending_txid).unwrap();
                self.tx_confirming_block(&txid).map(|b| SpendingInput {
                    txid,
                    vin: edge.key.spending_vin as u32,
                    confirmed: Some(b),
                })
            })
    }
    pub fn tx_confirming_block(&self, txid: &Txid) -> Option<BlockId> {
        let _timer = self.start_timer("tx_confirming_block");
        let headers = self.store.indexed_headers.read().unwrap();
        self.store
            .txstore_db
            .iter_scan(&TxConfRow::filter(&txid[..]))
            .map(TxConfRow::from_row)
            // header_by_blockhash only returns blocks that are part of the best chain,
            // or None for orphaned blocks.
            .filter_map(|conf| {
                headers.header_by_blockhash(&deserialize(&conf.key.blockhash).unwrap())
            })
            .next()
            .map(BlockId::from)
    }

    pub fn get_block_status(&self, hash: &BlockHash) -> BlockStatus {
        // TODO differentiate orphaned and non-existing blocks? telling them apart requires
        // an additional db read.

        let headers = self.store.indexed_headers.read().unwrap();

        // header_by_blockhash only returns blocks that are part of the best chain,
        // or None for orphaned blocks.
        headers
            .header_by_blockhash(hash)
            .map_or_else(BlockStatus::orphaned, |header| {
                BlockStatus::confirmed(
                    header.height(),
                    headers
                        .header_by_height(header.height() + 1)
                        .map(|h| *h.hash()),
                )
            })
    }

    pub fn get_merkleblock_proof(&self, txid: &Txid) -> Option<MerkleBlock> {
        let _timer = self.start_timer("get_merkleblock_proof");
        let blockid = self.tx_confirming_block(txid)?;
        let headerentry = self.header_by_hash(&blockid.hash)?;
        let block_txids = self.get_block_txids(&blockid.hash)?;
        let match_txids = vec![*txid].into_iter().collect();

        Some(MerkleBlock::from_header_txids(
            headerentry.header(),
            &block_txids,
            &match_txids,
        ))
    }
}

fn load_blockhashes(db: &DB, prefix: &[u8]) -> HashSet<BlockHash> {
    db.iter_scan(prefix)
        .map(BlockRow::from_row)
        .map(|r| deserialize(&r.key.hash).expect("failed to parse BlockHash"))
        .collect()
}

fn load_blockheaders(db: &DB) -> HashMap<BlockHash, BlockHeader> {
    db.iter_scan(&BlockRow::header_filter())
        .map(BlockRow::from_row)
        .map(|r| {
            let key: BlockHash = deserialize(&r.key.hash).expect("failed to parse BlockHash");
            let value: BlockHeader = deserialize(&r.value).expect("failed to parse BlockHeader");
            (key, value)
        })
        .collect()
}

fn add_blocks(block_entries: &[BlockEntry], iconfig: &IndexerConfig) -> Vec<DBRow> {
    // persist individual transactions:
    //      T{txid} → {rawtx}
    //      C{txid}{blockhash}{height} →
    //      O{txid}{index} → {txout}
    // persist block headers', block txids' and metadata rows:
    //      B{blockhash} → {header}
    //      X{blockhash} → {txid1}...{txidN}
    //      M{blockhash} → {tx_count}{size}{weight}
    block_entries
        .par_iter() // serialization is CPU-intensive
        .map(|b| {
            let mut rows = vec![];
            let blockhash = full_hash(&b.entry.hash()[..]);
            let txids: Vec<Txid> = b.block.txdata.iter().map(|tx| tx.malfix_txid()).collect();
            for tx in &b.block.txdata {
                add_transaction(tx, blockhash, &mut rows, iconfig);
            }

            if !iconfig.light_mode {
                rows.push(BlockRow::new_txids(blockhash, &txids).into_row());
                rows.push(BlockRow::new_meta(blockhash, &BlockMeta::from(b)).into_row());
            }

            rows.push(BlockRow::new_header(&b).into_row());
            rows.push(BlockRow::new_done(blockhash).into_row()); // mark block as "added"
            rows
        })
        .flatten()
        .collect()
}

fn add_transaction(
    tx: &Transaction,
    blockhash: FullHash,
    rows: &mut Vec<DBRow>,
    iconfig: &IndexerConfig,
) {
    rows.push(TxConfRow::new(tx, blockhash).into_row());

    if !iconfig.light_mode {
        rows.push(TxRow::new(tx).into_row());
    }

    let txid = full_hash(&tx.malfix_txid()[..]);
    for (txo_index, txo) in tx.output.iter().enumerate() {
        if is_spendable(txo) {
            rows.push(TxOutRow::new(&txid, txo_index, txo).into_row());
        }
    }
}

fn get_previous_txos(block_entries: &[BlockEntry]) -> BTreeSet<OutPoint> {
    block_entries
        .iter()
        .flat_map(|b| b.block.txdata.iter())
        .flat_map(|tx| {
            tx.input
                .iter()
                .filter(|txin| has_prevout(txin))
                .map(|txin| txin.previous_output)
        })
        .collect()
}

fn lookup_txos(
    txstore_db: &DB,
    outpoints: &BTreeSet<OutPoint>,
    allow_missing: bool,
) -> HashMap<OutPoint, TxOut> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(16) // we need to saturate SSD IOPS
        .thread_name(|i| format!("lookup-txo-{}", i))
        .build()
        .unwrap();
    pool.install(|| {
        outpoints
            .par_iter()
            .filter_map(|outpoint| {
                lookup_txo(&txstore_db, &outpoint)
                    .or_else(|| {
                        if !allow_missing {
                            panic!("missing txo {} in {:?}", outpoint, txstore_db);
                        }
                        None
                    })
                    .map(|txo| (*outpoint, txo))
            })
            .collect()
    })
}

fn lookup_txo(txstore_db: &DB, outpoint: &OutPoint) -> Option<TxOut> {
    txstore_db
        .get(&TxOutRow::key(&outpoint))
        .map(|val| deserialize(&val).expect("failed to parse TxOut"))
}

fn index_blocks(
    block_entries: &[BlockEntry],
    previous_txos_map: &HashMap<OutPoint, TxOut>,
    iconfig: &IndexerConfig,
) -> Vec<DBRow> {
    block_entries
        .par_iter() // serialization is CPU-intensive
        .map(|b| {
            let mut rows = vec![];
            for tx in &b.block.txdata {
                let height = b.entry.height() as u32;
                index_transaction(tx, height, previous_txos_map, &mut rows, iconfig);
            }
            rows.push(BlockRow::new_done(full_hash(&b.entry.hash()[..])).into_row()); // mark block as "indexed"
            rows
        })
        .flatten()
        .collect()
}

// TODO: return an iterator?
fn index_transaction(
    tx: &Transaction,
    confirmed_height: u32,
    previous_txos_map: &HashMap<OutPoint, TxOut>,
    rows: &mut Vec<DBRow>,
    iconfig: &IndexerConfig,
) {
    // persist history index:
    //      H{funding-scripthash}{funding-height}F{funding-txid:vout} → ""
    //      H{funding-scripthash}{spending-height}S{spending-txid:vin}{funding-txid:vout} → ""
    // persist "edges" for fast is-this-TXO-spent check
    //      S{funding-txid:vout}{spending-txid:vin} → ""
    let txid = full_hash(&tx.malfix_txid()[..]);
    for (txo_index, txo) in tx.output.iter().enumerate() {
        if is_spendable(txo) || iconfig.index_unspendables {
            if let Some((color_id, script)) = txo.script_pubkey.split_color() {
                let history = TxHistoryRow::new(
                    &txo.script_pubkey,
                    confirmed_height,
                    TxHistoryInfo::Funding(FundingInfo {
                        txid,
                        vout: txo_index as u16,
                        color_id: color_id.clone(),
                        value: txo.value,
                        open_asset: None,
                    }),
                );
                rows.push(history.into_row());
                let history = TxHistoryRow::new(
                    &script,
                    confirmed_height,
                    TxHistoryInfo::Funding(FundingInfo {
                        txid,
                        vout: txo_index as u16,
                        color_id: color_id.clone(),
                        value: txo.value,
                        open_asset: None,
                    }),
                );
                rows.push(history.into_row());
            } else {
                let history = TxHistoryRow::new(
                    &txo.script_pubkey,
                    confirmed_height,
                    TxHistoryInfo::Funding(FundingInfo {
                        txid,
                        vout: txo_index as u16,
                        color_id: ColorIdentifier::default(),
                        value: txo.value,
                        open_asset: None,
                    }),
                );
                rows.push(history.into_row());
            }

            if iconfig.address_search {
                if let Some(row) = addr_search_row(&txo.script_pubkey, iconfig.network) {
                    rows.push(row);
                }
            }
        }
    }
    for (txi_index, txi) in tx.input.iter().enumerate() {
        if !has_prevout(txi) {
            continue;
        }
        let prev_txo = previous_txos_map
            .get(&txi.previous_output)
            .unwrap_or_else(|| panic!("missing previous txo {}", txi.previous_output));

        let color_id = prev_txo
            .script_pubkey
            .split_color()
            .map(|(color_id, _)| color_id)
            .unwrap_or(ColorIdentifier::default());

        let history = TxHistoryRow::new(
            &prev_txo.script_pubkey,
            confirmed_height,
            TxHistoryInfo::Spending(SpendingInfo {
                txid,
                vin: txi_index as u16,
                prev_txid: full_hash(&txi.previous_output.txid[..]),
                prev_vout: txi.previous_output.vout as u16,
                color_id: color_id,
                value: prev_txo.value,
            }),
        );
        rows.push(history.into_row());

        let edge = TxEdgeRow::new(
            full_hash(&txi.previous_output.txid[..]),
            txi.previous_output.vout as u16,
            txid,
            txi_index as u16,
        );
        rows.push(edge.into_row());
    }
}

fn addr_search_row(spk: &Script, network: Network) -> Option<DBRow> {
    script_to_address(spk, network).map(|address| DBRow {
        key: [b"a", address.as_bytes()].concat(),
        value: vec![],
    })
}

fn addr_search_filter(prefix: &str) -> Bytes {
    [b"a", prefix.as_bytes()].concat()
}

// TODO: replace by a separate opaque type (similar to Sha256dHash, but without the "double")
pub type FullHash = [u8; 32]; // serialized SHA256 result

pub fn compute_script_hash(script: &Script) -> FullHash {
    let mut hash = FullHash::default();
    let mut sha2 = Sha256::new();
    sha2.input(script.as_bytes());
    sha2.result(&mut hash);
    hash
}

pub fn parse_hash(hash: &FullHash) -> Sha256dHash {
    deserialize(hash).expect("failed to parse Sha256dHash")
}

#[derive(Serialize, Deserialize)]
struct TxRowKey {
    code: u8,
    txid: FullHash,
}

struct TxRow {
    key: TxRowKey,
    value: Bytes, // raw transaction
}

impl TxRow {
    fn new(txn: &Transaction) -> TxRow {
        let txid = full_hash(&txn.malfix_txid()[..]);
        TxRow {
            key: TxRowKey { code: b'T', txid },
            value: serialize(txn),
        }
    }

    fn key(prefix: &[u8]) -> Bytes {
        [b"T", prefix].concat()
    }

    fn into_row(self) -> DBRow {
        let TxRow { key, value } = self;
        DBRow {
            key: bincode::serialize(&key).unwrap(),
            value,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct TxConfKey {
    code: u8,
    txid: FullHash,
    blockhash: FullHash,
}

struct TxConfRow {
    key: TxConfKey,
}

impl TxConfRow {
    fn new(txn: &Transaction, blockhash: FullHash) -> TxConfRow {
        let txid = full_hash(&txn.malfix_txid()[..]);
        TxConfRow {
            key: TxConfKey {
                code: b'C',
                txid,
                blockhash,
            },
        }
    }

    fn filter(prefix: &[u8]) -> Bytes {
        [b"C", prefix].concat()
    }

    fn into_row(self) -> DBRow {
        DBRow {
            key: bincode::serialize(&self.key).unwrap(),
            value: vec![],
        }
    }

    fn from_row(row: DBRow) -> Self {
        TxConfRow {
            key: bincode::deserialize(&row.key).expect("failed to parse TxConfKey"),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct TxOutKey {
    code: u8,
    txid: FullHash,
    vout: u16,
}

struct TxOutRow {
    key: TxOutKey,
    value: Bytes, // serialized output
}

impl TxOutRow {
    fn new(txid: &FullHash, vout: usize, txout: &TxOut) -> TxOutRow {
        TxOutRow {
            key: TxOutKey {
                code: b'O',
                txid: *txid,
                vout: vout as u16,
            },
            value: serialize(txout),
        }
    }
    fn key(outpoint: &OutPoint) -> Bytes {
        bincode::serialize(&TxOutKey {
            code: b'O',
            txid: full_hash(&outpoint.txid[..]),
            vout: outpoint.vout as u16,
        })
        .unwrap()
    }

    fn into_row(self) -> DBRow {
        DBRow {
            key: bincode::serialize(&self.key).unwrap(),
            value: self.value,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct BlockKey {
    code: u8,
    hash: FullHash,
}

struct BlockRow {
    key: BlockKey,
    value: Bytes, // serialized output
}

impl BlockRow {
    fn new_header(block_entry: &BlockEntry) -> BlockRow {
        BlockRow {
            key: BlockKey {
                code: b'B',
                hash: full_hash(&block_entry.entry.hash()[..]),
            },
            value: serialize(&block_entry.block.header),
        }
    }

    fn new_txids(hash: FullHash, txids: &[Txid]) -> BlockRow {
        BlockRow {
            key: BlockKey { code: b'X', hash },
            value: bincode::serialize(txids).unwrap(),
        }
    }

    fn new_meta(hash: FullHash, meta: &BlockMeta) -> BlockRow {
        BlockRow {
            key: BlockKey { code: b'M', hash },
            value: bincode::serialize(meta).unwrap(),
        }
    }

    fn new_done(hash: FullHash) -> BlockRow {
        BlockRow {
            key: BlockKey { code: b'D', hash },
            value: vec![],
        }
    }

    fn header_filter() -> Bytes {
        b"B".to_vec()
    }

    fn txids_key(hash: FullHash) -> Bytes {
        [b"X", &hash[..]].concat()
    }

    fn meta_key(hash: FullHash) -> Bytes {
        [b"M", &hash[..]].concat()
    }

    fn done_filter() -> Bytes {
        b"D".to_vec()
    }

    fn into_row(self) -> DBRow {
        DBRow {
            key: bincode::serialize(&self.key).unwrap(),
            value: self.value,
        }
    }

    fn from_row(row: DBRow) -> Self {
        BlockRow {
            key: bincode::deserialize(&row.key).unwrap(),
            value: row.value,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FundingInfo {
    pub txid: FullHash,
    pub vout: u16,
    pub color_id: ColorIdentifier,
    pub value: Value,
    #[serde(skip)]
    pub open_asset: Option<OpenAsset>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpendingInfo {
    pub txid: FullHash, // spending transaction
    pub vin: u16,
    pub prev_txid: FullHash, // funding transaction
    pub prev_vout: u16,
    pub color_id: ColorIdentifier,
    pub value: Value,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TxHistoryInfo {
    Funding(FundingInfo),
    Spending(SpendingInfo),
}

impl TxHistoryInfo {
    pub fn get_txid(&self) -> Txid {
        match self {
            TxHistoryInfo::Funding(FundingInfo { txid, .. })
            | TxHistoryInfo::Spending(SpendingInfo { txid, .. }) => deserialize(txid),
        }
        .expect("cannot parse Txid")
    }

    pub fn get_funded_outpoint(&self) -> OutPoint {
        match self {
            TxHistoryInfo::Funding(ref info) => OutPoint {
                txid: deserialize(&info.txid).unwrap(),
                vout: info.vout as u32,
            },
            TxHistoryInfo::Spending(ref info) => OutPoint {
                txid: deserialize(&info.prev_txid).unwrap(),
                vout: info.prev_vout as u32,
            },
        }
    }

    pub fn color_id(&self) -> ColorIdentifier {
        match self {
            TxHistoryInfo::Funding(ref info) => info.color_id.clone(),
            TxHistoryInfo::Spending(ref info) => info.color_id.clone(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct TxHistoryKey {
    pub code: u8,              // H for script history or I for asset history (elements only)
    pub hash: FullHash, // either a scripthash (always on tapyrus) or an asset id (elements only)
    pub confirmed_height: u32, // MUST be serialized as big-endian (for correct scans).
    pub txinfo: TxHistoryInfo,
}

pub struct TxHistoryRow {
    pub key: TxHistoryKey,
}

impl TxHistoryRow {
    fn new(script: &Script, confirmed_height: u32, txinfo: TxHistoryInfo) -> Self {
        let key = TxHistoryKey {
            code: b'H',
            hash: compute_script_hash(&script),
            confirmed_height,
            txinfo,
        };
        TxHistoryRow { key }
    }

    fn filter(code: u8, hash_prefix: &[u8]) -> Bytes {
        [&[code], hash_prefix].concat()
    }

    fn prefix_end(code: u8, hash: &[u8]) -> Bytes {
        bincode::serialize(&(code, full_hash(&hash[..]), std::u32::MAX)).unwrap()
    }

    fn prefix_height(code: u8, hash: &[u8], height: u32) -> Bytes {
        bincode::options()
            .with_big_endian()
            .serialize(&(code, full_hash(&hash[..]), height))
            .unwrap()
    }

    pub fn into_row(self) -> DBRow {
        DBRow {
            key: bincode::options()
                .with_big_endian()
                .serialize(&self.key)
                .unwrap(),
            value: vec![],
        }
    }

    pub fn from_row(row: DBRow) -> Self {
        let key = bincode::options()
            .with_big_endian()
            .deserialize(&row.key)
            .expect("failed to deserialize TxHistoryKey");
        TxHistoryRow { key }
    }

    pub fn get_txid(&self) -> Txid {
        self.key.txinfo.get_txid()
    }

    fn get_funded_outpoint(&self) -> OutPoint {
        self.key.txinfo.get_funded_outpoint()
    }
}

#[derive(Serialize, Deserialize)]
struct TxEdgeKey {
    code: u8,
    funding_txid: FullHash,
    funding_vout: u16,
    spending_txid: FullHash,
    spending_vin: u16,
}

struct TxEdgeRow {
    key: TxEdgeKey,
}

impl TxEdgeRow {
    fn new(
        funding_txid: FullHash,
        funding_vout: u16,
        spending_txid: FullHash,
        spending_vin: u16,
    ) -> Self {
        let key = TxEdgeKey {
            code: b'S',
            funding_txid,
            funding_vout,
            spending_txid,
            spending_vin,
        };
        TxEdgeRow { key }
    }

    fn filter(outpoint: &OutPoint) -> Bytes {
        // TODO build key without using bincode? [ b"S", &outpoint.txid[..], outpoint.vout?? ].concat()
        bincode::serialize(&(b'S', full_hash(&outpoint.txid[..]), outpoint.vout as u16)).unwrap()
    }

    fn into_row(self) -> DBRow {
        DBRow {
            key: bincode::serialize(&self.key).unwrap(),
            value: vec![],
        }
    }

    fn from_row(row: DBRow) -> Self {
        TxEdgeRow {
            key: bincode::deserialize(&row.key).expect("failed to deserialize TxEdgeKey"),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ScriptCacheKey {
    code: u8,
    scripthash: FullHash,
}

struct StatsCacheRow {
    key: StatsCacheKey,
    value: Bytes,
}

#[derive(Serialize, Deserialize)]
struct StatsCacheKey {
    code: u8,
    scripthash: FullHash,
    color_id: ColorIdentifier,
}

impl StatsCacheRow {
    fn new(
        scripthash: &[u8],
        color_id: ColorIdentifier,
        stats: &ScriptStats,
        blockhash: &BlockHash,
    ) -> Self {
        StatsCacheRow {
            key: StatsCacheKey {
                code: b'A',
                scripthash: full_hash(scripthash),
                color_id: color_id,
            },
            value: bincode::serialize(&(stats, blockhash)).unwrap(),
        }
    }

    pub fn key(scripthash: &[u8]) -> Bytes {
        [b"A", scripthash].concat()
    }

    pub fn prefix_color_id(scripthash: &[u8], color_id: ColorIdentifier) -> Bytes {
        bincode::options()
            .with_big_endian()
            .serialize(&(b"A", &scripthash[..], b"C", &color_id))
            .unwrap()
    }

    fn into_row(self) -> DBRow {
        DBRow {
            key: bincode::serialize(&self.key).unwrap(),
            value: self.value,
        }
    }

    pub fn from_row(row: DBRow) -> Self {
        let key = bincode::deserialize(&row.key).expect("failed to deserialize StatsCacheKey");
        StatsCacheRow {
            key,
            value: row.value,
        }
    }
}

type CachedUtxoMap = HashMap<(Txid, u32), (u32, ColorIdentifier, Value)>; // (txid,vout) => (block_height, color_id, output_value)

struct UtxoCacheRow {
    key: ScriptCacheKey,
    value: Bytes,
}

impl UtxoCacheRow {
    fn new(scripthash: &[u8], utxos: &UtxoMap, blockhash: &BlockHash) -> Self {
        let utxos_cache = make_utxo_cache(utxos);

        UtxoCacheRow {
            key: ScriptCacheKey {
                code: b'U',
                scripthash: full_hash(scripthash),
            },
            value: bincode::serialize(&(utxos_cache, blockhash)).unwrap(),
        }
    }

    pub fn key(scripthash: &[u8]) -> Bytes {
        [b"U", scripthash].concat()
    }

    fn into_row(self) -> DBRow {
        DBRow {
            key: bincode::serialize(&self.key).unwrap(),
            value: self.value,
        }
    }
}

// keep utxo cache with just the block height (the hash/timestamp are read later from the headers to reconstruct BlockId)
// and use a (txid,vout) tuple instead of OutPoints (they don't play nicely with bincode serialization)
fn make_utxo_cache(utxos: &UtxoMap) -> CachedUtxoMap {
    utxos
        .iter()
        .map(|(outpoint, (blockid, color_id, value))| {
            (
                (outpoint.txid, outpoint.vout),
                (blockid.height as u32, color_id.clone(), *value),
            )
        })
        .collect()
}

fn from_utxo_cache(utxos_cache: CachedUtxoMap, chain: &ChainQuery) -> UtxoMap {
    utxos_cache
        .into_iter()
        .map(|((txid, vout), (height, color_id, value))| {
            let outpoint = OutPoint { txid, vout };
            let blockid = chain
                .blockid_by_height(height as usize)
                .expect("missing blockheader for valid utxo cache entry");
            (outpoint, (blockid, color_id, value))
        })
        .collect()
}

pub fn update_stats(
    init_stats: StatsMap,
    histories: &Vec<(TxHistoryInfo, Option<BlockId>)>,
) -> (StatsMap, Option<BlockHash>) {
    let mut stats = init_stats;
    let mut seen_txids_map: HashMap<ColorIdentifier, HashSet<Txid>> = HashMap::new();
    let mut lastblock = None;

    for (history, blockid_opt) in histories {
        let color_id: ColorIdentifier = history.color_id();
        let mut seen_txids = match seen_txids_map.get(&color_id) {
            Some(seen_txids) => seen_txids.clone(),
            None => HashSet::new(),
        };
        if lastblock != blockid_opt.clone().map(|blockid| blockid.hash) {
            seen_txids.clear();
        }

        match stats.get_mut(&color_id) {
            Some(s) => _update_stats(s, &mut seen_txids, &history),
            None => {
                let mut s = ScriptStats::default();
                _update_stats(&mut s, &mut seen_txids, &history);
                stats.insert(color_id.clone(), s);
            }
        }
        seen_txids_map.insert(color_id, seen_txids);
        lastblock = blockid_opt.clone().map(|blockid| blockid.hash);
    }
    (stats, lastblock)
}

fn _update_stats(stat: &mut ScriptStats, seen_txids: &mut HashSet<Txid>, entry: &TxHistoryInfo) {
    if seen_txids.insert(entry.get_txid()) {
        stat.tx_count += 1;
    }

    match entry {
        TxHistoryInfo::Funding(info) => {
            stat.funded_txo_count += 1;
            stat.funded_txo_sum += info.value;
        }
        TxHistoryInfo::Spending(info) => {
            stat.spent_txo_count += 1;
            stat.spent_txo_sum += info.value;
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_update_stats_for_chain() {
        let stats = StatsMap::new();

        let funding_txid =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000000")
                .unwrap();
        let spending_txid =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap();

        let blockhash1 = deserialize(
            &hex::decode("0000000000000000000000000000000000000000000000000000000000000011")
                .unwrap(),
        )
        .unwrap();
        let blockhash2 = deserialize(
            &hex::decode("0000000000000000000000000000000000000000000000000000000000000012")
                .unwrap(),
        )
        .unwrap();

        let funding = (
            TxHistoryInfo::Funding(FundingInfo {
                txid: full_hash(&funding_txid),
                vout: 0,
                color_id: ColorIdentifier::default(),
                value: 100,
                open_asset: None,
            }),
            Some(BlockId {
                height: 1,
                hash: blockhash1,
                time: 0,
            }),
        );

        let spending = (
            TxHistoryInfo::Spending(SpendingInfo {
                txid: full_hash(&spending_txid),
                vin: 0,
                prev_txid: full_hash(&funding_txid),
                prev_vout: 0,
                color_id: ColorIdentifier::default(),
                value: 100,
            }),
            Some(BlockId {
                height: 2,
                hash: blockhash2,
                time: 0,
            }),
        );

        let (newstats, latestblock) = update_stats(stats, &vec![funding, spending]);
        assert_eq!(newstats.len(), 1);

        let stat: &ScriptStats = newstats.values().nth(0).unwrap();
        assert_eq!(stat.tx_count, 2);
        assert_eq!(stat.funded_txo_count, 1);
        assert_eq!(stat.funded_txo_sum, 100);
        assert_eq!(stat.spent_txo_count, 1);
        assert_eq!(stat.spent_txo_sum, 100);
        assert_eq!(latestblock, Some(blockhash2));
    }

    #[test]
    fn test_update_stats_for_mempool() {
        let stats = StatsMap::new();

        let funding_txid =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000000")
                .unwrap();
        let spending_txid =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap();

        let funding = (
            TxHistoryInfo::Funding(FundingInfo {
                txid: full_hash(&funding_txid),
                vout: 0,
                color_id: ColorIdentifier::default(),
                value: 100,
                open_asset: None,
            }),
            None,
        );

        let spending = (
            TxHistoryInfo::Spending(SpendingInfo {
                txid: full_hash(&spending_txid),
                vin: 0,
                prev_txid: full_hash(&funding_txid),
                prev_vout: 0,
                color_id: ColorIdentifier::default(),
                value: 100,
            }),
            None,
        );

        let (newstats, latestblock) = update_stats(stats, &vec![funding, spending]);
        assert_eq!(newstats.len(), 1);

        let stat: &ScriptStats = newstats.values().nth(0).unwrap();
        assert_eq!(stat.tx_count, 2);
        assert_eq!(stat.funded_txo_count, 1);
        assert_eq!(stat.funded_txo_sum, 100);
        assert_eq!(stat.spent_txo_count, 1);
        assert_eq!(stat.spent_txo_sum, 100);
        assert_eq!(latestblock, None);
    }

    #[test]
    fn test_update_stats_colored() {
        let stats = StatsMap::new();

        let txid1 = hex::decode("0000000000000000000000000000000000000000000000000000000000000000")
            .unwrap();
        let txid2 = hex::decode("0000000000000000000000000000000000000000000000000000000000000001")
            .unwrap();

        let funding1 = (
            TxHistoryInfo::Funding(FundingInfo {
                txid: full_hash(&txid1),
                vout: 0,
                color_id: ColorIdentifier::default(),
                value: 100,
                open_asset: None,
            }),
            None,
        );

        let spending1 = (
            TxHistoryInfo::Spending(SpendingInfo {
                txid: full_hash(&txid2),
                vin: 0,
                prev_txid: full_hash(&txid1),
                prev_vout: 0,
                color_id: ColorIdentifier::default(),
                value: 100,
            }),
            None,
        );

        let out_point = tapyrus::OutPoint::new(deserialize(&txid1).unwrap(), 0);
        let color_id = ColorIdentifier::nft(out_point);

        let funding2 = (
            TxHistoryInfo::Funding(FundingInfo {
                txid: full_hash(&txid2),
                vout: 0,
                color_id: color_id.clone(),
                value: 200,
                open_asset: None,
            }),
            None,
        );

        let (newstats, latestblock) = update_stats(stats, &vec![funding1, spending1, funding2]);
        assert_eq!(newstats.len(), 2);

        let stat: &ScriptStats = newstats.get(&ColorIdentifier::default()).unwrap();
        assert_eq!(stat.tx_count, 2);
        assert_eq!(stat.funded_txo_count, 1);
        assert_eq!(stat.funded_txo_sum, 100);
        assert_eq!(stat.spent_txo_count, 1);
        assert_eq!(stat.spent_txo_sum, 100);
        assert_eq!(latestblock, None);

        let stat: &ScriptStats = newstats.get(&color_id).unwrap();
        assert_eq!(stat.tx_count, 1);
        assert_eq!(stat.funded_txo_count, 1);
        assert_eq!(stat.funded_txo_sum, 200);
        assert_eq!(stat.spent_txo_count, 0);
        assert_eq!(stat.spent_txo_sum, 0);
        assert_eq!(latestblock, None);
    }
}
