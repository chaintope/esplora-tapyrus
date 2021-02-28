use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::mpsc::{Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;

use crypto::digest::Digest;
use crypto::sha2::Sha256;
use error_chain::ChainedError;
use hex;
use serde_json::{from_str, Value};
use tapyrus::blockdata::block::BlockHeader;
use tapyrus::blockdata::script::{ColorIdentifier, Script};
use tapyrus::blockdata::transaction::{OutPoint, Transaction, TxIn, TxOut};
use tapyrus::consensus::encode::{deserialize, serialize};
use tapyrus::hashes::sha256d::Hash as Sha256dHash;
use tapyrus::util::amount::Amount;
use tapyrus::Txid;

use crate::config::Config;
use crate::electrum::{get_electrum_height, ProtocolVersion};
use crate::errors::*;
use crate::metrics::{Gauge, HistogramOpts, HistogramVec, MetricOpts, Metrics};
use crate::new_index::schema::{ScriptStats, StatsMap};
use crate::new_index::Query;
use crate::new_index::Utxo;
use crate::open_assets::OpenAsset;
use crate::util::electrum_merkle::{get_header_merkle_proof, get_id_from_pos, get_tx_merkle_proof};
use crate::util::{
    create_socket, full_hash, spawn_thread, BlockId, BoolThen, Channel, FullHash, HeaderEntry,
    SyncChannel,
};

const ELECTRS_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::new(1, 4);
const MAX_HEADERS: usize = 2016;

// TODO: Sha256dHash should be a generic hash-container (since script hash is single SHA256)
fn hash_from_value(val: Option<&Value>) -> Result<Sha256dHash> {
    let script_hash = val.chain_err(|| "missing hash")?;
    let script_hash = script_hash.as_str().chain_err(|| "non-string hash")?;
    let script_hash = script_hash.parse().chain_err(|| "non-hex hash")?;
    Ok(script_hash)
}

fn usize_from_value(val: Option<&Value>, name: &str) -> Result<usize> {
    let val = val.chain_err(|| format!("missing {}", name))?;
    let val = val.as_u64().chain_err(|| format!("non-integer {}", name))?;
    Ok(val as usize)
}

fn usize_from_value_or(val: Option<&Value>, name: &str, default: usize) -> Result<usize> {
    if val.is_none() {
        return Ok(default);
    }
    usize_from_value(val, name)
}

fn bool_from_value(val: Option<&Value>, name: &str) -> Result<bool> {
    let val = val.chain_err(|| format!("missing {}", name))?;
    let val = val.as_bool().chain_err(|| format!("not a bool {}", name))?;
    Ok(val)
}

fn bool_from_value_or(val: Option<&Value>, name: &str, default: bool) -> Result<bool> {
    if val.is_none() {
        return Ok(default);
    }
    bool_from_value(val, name)
}

fn color_id_from_value(val: Option<&Value>, name: &str) -> Result<Option<ColorIdentifier>> {
    if let Some(val) = val {
        let val = val
            .as_str()
            .chain_err(|| format!("not a string {}", name))?;
        let val = hex::decode(val).chain_err(|| format!("invalie hex format: {}", name))?;
        let color_id: ColorIdentifier =
            deserialize(&val[0..33]).chain_err(|| format!("decode failed: {}", name))?;
        Ok(Some(color_id))
    } else {
        Ok(None)
    }
}

// TODO: implement caching and delta updates
fn get_status_hash(txs: Vec<(Txid, Option<BlockId>)>, query: &Query) -> Option<FullHash> {
    if txs.is_empty() {
        None
    } else {
        let mut hash = FullHash::default();
        let mut sha2 = Sha256::new();
        for (txid, blockid) in txs {
            let is_mempool = blockid.is_none();
            let has_unconfirmed_parents = is_mempool
                .and_then(|| Some(query.has_unconfirmed_parents(&txid)))
                .unwrap_or(false);
            let height = get_electrum_height(blockid, has_unconfirmed_parents);
            let part = format!("{}:{}:", txid, height);
            sha2.input(part.as_bytes());
        }
        sha2.result(&mut hash);
        Some(hash)
    }
}

struct Connection {
    query: Arc<Query>,
    last_header_entry: Option<HeaderEntry>,
    status_hashes: HashMap<Sha256dHash, Value>, // ScriptHash -> StatusHash
    stream: TcpStream,
    addr: SocketAddr,
    chan: SyncChannel<Message>,
    stats: Arc<Stats>,
    txs_limit: usize,
    enable_open_assets: bool,
}

impl Connection {
    pub fn new(
        query: Arc<Query>,
        stream: TcpStream,
        addr: SocketAddr,
        stats: Arc<Stats>,
        txs_limit: usize,
        enable_open_assets: bool,
    ) -> Connection {
        Connection {
            query,
            last_header_entry: None, // disable header subscription for now
            status_hashes: HashMap::new(),
            stream,
            addr,
            chan: SyncChannel::new(10),
            stats,
            txs_limit,
            enable_open_assets,
        }
    }

    fn blockchain_headers_subscribe(&mut self) -> Result<Value> {
        let entry = self.query.chain().best_header();
        let hex_header = hex::encode(serialize(entry.header()));
        let result = json!({"hex": hex_header, "height": entry.height()});
        self.last_header_entry = Some(entry);
        Ok(result)
    }

    fn server_version(&self) -> Result<Value> {
        Ok(json!([
            format!("electrs-esplora {}", ELECTRS_VERSION),
            PROTOCOL_VERSION
        ]))
    }

    fn server_banner(&self) -> Result<Value> {
        Ok(json!(self.query.config().electrum_banner.clone()))
    }

    fn server_donation_address(&self) -> Result<Value> {
        Ok(Value::Null)
    }

    fn server_peers_subscribe(&self) -> Result<Value> {
        let servers = json!([]);

        Ok(servers)
    }

    fn mempool_get_fee_histogram(&self) -> Result<Value> {
        Ok(json!(&self.query.mempool().backlog_stats().fee_histogram))
    }

    fn blockchain_block_header(&self, params: &[Value]) -> Result<Value> {
        let height = usize_from_value(params.get(0), "height")?;
        let cp_height = usize_from_value_or(params.get(1), "cp_height", 0)?;

        let raw_header_hex: String = self
            .query
            .chain()
            .header_by_height(height)
            .map(|entry| hex::encode(&serialize(entry.header())))
            .chain_err(|| "missing header")?;

        if cp_height == 0 {
            return Ok(json!(raw_header_hex));
        }
        let (branch, root) = get_header_merkle_proof(self.query.chain(), height, cp_height)?;

        Ok(json!({
            "header": raw_header_hex,
            "root": root,
            "branch": branch
        }))
    }

    fn blockchain_block_headers(&self, params: &[Value]) -> Result<Value> {
        let start_height = usize_from_value(params.get(0), "start_height")?;
        let count = MAX_HEADERS.min(usize_from_value(params.get(1), "count")?);
        let cp_height = usize_from_value_or(params.get(2), "cp_height", 0)?;
        let heights: Vec<usize> = (start_height..(start_height + count)).collect();
        let headers: Vec<String> = heights
            .into_iter()
            .filter_map(|height| {
                self.query
                    .chain()
                    .header_by_height(height)
                    .map(|entry| hex::encode(&serialize(entry.header())))
            })
            .collect();

        if count == 0 || cp_height == 0 {
            return Ok(json!({
                "count": headers.len(),
                "hex": headers.join(""),
                "max": MAX_HEADERS,
            }));
        }

        let (branch, root) =
            get_header_merkle_proof(self.query.chain(), start_height + (count - 1), cp_height)?;

        Ok(json!({
            "count": headers.len(),
            "hex": headers.join(""),
            "max": MAX_HEADERS,
            "root": root,
            "branch" : branch,
        }))
    }

    fn blockchain_estimatefee(&self, params: &[Value]) -> Result<Value> {
        let conf_target = usize_from_value(params.get(0), "blocks_count")?;
        let fee_rate = self
            .query
            .estimate_fee(conf_target as u16)
            .chain_err(|| format!("cannot estimate fee for {} blocks", conf_target))?;
        // convert from tapyrus/b to TPC/kB, as expected by Electrum clients
        Ok(json!(fee_rate / 100_000f64))
    }

    fn utxo_to_json(&self, utxo: &Utxo, asset: Option<&OpenAsset>) -> Value {
        if let Some(asset) = asset {
            json!({
                "height": utxo.confirmed.clone().map_or(0, |b| b.height),
                "tx_pos": utxo.vout,
                "tx_hash": utxo.txid,
                "value": utxo.value,
                "asset": {
                    "asset_id": asset.asset_id.to_string(),
                    "asset_quantity": asset.asset_quantity
                }
            })
        } else if utxo.color_id.is_colored() {
            json!({
                "height": utxo.confirmed.clone().map_or(0, |b| b.height),
                "tx_pos": utxo.vout,
                "tx_hash": utxo.txid,
                "color_id": utxo.color_id,
                "value": utxo.value,
            })
        } else {
            json!({
                "height": utxo.confirmed.clone().map_or(0, |b| b.height),
                "tx_pos": utxo.vout,
                "tx_hash": utxo.txid,
                "value": utxo.value,
            })
        }
    }

    fn balance_to_json(
        &self,
        chain_stats: &StatsMap,
        mempool_stats: &StatsMap,
        color_id: ColorIdentifier,
    ) -> Value {
        let initial_stat = ScriptStats::default();
        let chain = chain_stats.get(&color_id).unwrap_or(&initial_stat);
        let mempool = mempool_stats.get(&color_id).unwrap_or(&initial_stat);
        if color_id.is_colored() {
            json!({
                "color_id": color_id,
                "confirmed": chain.funded_txo_sum - chain.spent_txo_sum,
                "unconfirmed": mempool.funded_txo_sum as i64 - mempool.spent_txo_sum as i64,
            })
        } else {
            json!({
                "confirmed": chain.funded_txo_sum - chain.spent_txo_sum,
                "unconfirmed": mempool.funded_txo_sum as i64 - mempool.spent_txo_sum as i64,
            })
        }
    }

    fn script_sig_to_json(&self, script_sig: &Script) -> Value {
        json!({
            "asm": script_sig.asm(),
            "hex": format!("{:x}", script_sig)
        })
    }

    fn input_to_json(&self, input: &TxIn) -> Value {
        json!({
            "scriptSig": self.script_sig_to_json(&input.script_sig),
            "sequence": input.sequence,
            "txid": input.previous_output.txid,
            "vout": input.previous_output.vout,
        })
    }

    fn script_pubkey_to_json(&self, script_pubkey: &Script) -> Value {
        // TODO: `reqSigs` and `addresses` fields should be added.
        json!({
            "asm": script_pubkey.asm(),
            "hex": format!("{:x}", script_pubkey),
            "type": script_pubkey.type_string()
        })
    }

    fn output_to_json(&self, index: usize, output: &TxOut) -> Value {
        json!({
            "n": index,
            "scriptPubKey": self.script_pubkey_to_json(&output.script_pubkey),
            "value": Amount::from_tap(output.value).as_tpc(),
        })
    }

    /// Write JSON for transaction data
    /// see https://electrumx.readthedocs.io/en/latest/protocol-methods.html#blockchain-transaction-get
    fn transaction_to_json(&self, transaction: &Transaction, confirmations: usize, header: &BlockHeader) -> Value {
        let ser = serialize(transaction);
        json!({
            "blockhash": header.block_hash(),
            "blocktime": header.time,
            "confirmations": confirmations,
            "hash": transaction.malfix_txid(),
            "hex": hex::encode(ser),
            "locktime": transaction.lock_time,
            "size": transaction.get_size(),
            "time": header.time,
            "txid": transaction.malfix_txid(),
            "version": transaction.version,
            "vin": transaction.input.iter().map(|input| self.input_to_json(&input)).collect::<Value>(),
            "vout": transaction.output.iter().enumerate().map(|(i, output)| self.output_to_json(i, &output)).collect::<Value>(),
        })
    }

    fn blockchain_openassets_scripthash_listunspent(&self, params: &[Value]) -> Result<Value> {
        let script_hash = hash_from_value(params.get(0)).chain_err(|| "bad script_hash")?;
        let utxos = self.query.utxo(&script_hash[..])?;
        let assets = self.query.open_assets(&utxos)?;
        Ok(json!(Value::Array(
            utxos
                .into_iter()
                .map(|utxo| {
                    let asset = assets.get(&OutPoint::new(utxo.txid, utxo.vout));
                    (utxo, asset)
                })
                .map(|(utxo, asset)| self.utxo_to_json(&utxo, asset))
                .collect()
        )))
    }

    fn blockchain_openassets_scripthash_listcoloredunspent(
        &self,
        params: &[Value],
    ) -> Result<Value> {
        let script_hash = hash_from_value(params.get(0)).chain_err(|| "bad script_hash")?;
        let utxos = self.query.utxo(&script_hash[..])?;
        let assets = self.query.open_assets(&utxos)?;
        Ok(json!(Value::Array(
            utxos
                .into_iter()
                .map(|utxo| {
                    let asset = assets.get(&OutPoint::new(utxo.txid, utxo.vout));
                    (utxo, asset)
                })
                .filter_map(|(utxo, asset_opt)| match asset_opt {
                    Some(_) => {
                        Some((utxo, asset_opt))
                    }
                    None => None,
                })
                .map(|(utxo, asset)| self.utxo_to_json(&utxo, asset))
                .collect()
        )))
    }

    fn blockchain_openassets_scripthash_listuncoloredunspent(
        &self,
        params: &[Value],
    ) -> Result<Value> {
        let script_hash = hash_from_value(params.get(0)).chain_err(|| "bad script_hash")?;
        let utxos = self.query.utxo(&script_hash[..])?;
        let assets = self.query.open_assets(&utxos)?;
        Ok(json!(Value::Array(
            utxos
                .into_iter()
                .map(|utxo| {
                    let asset = assets.get(&OutPoint::new(utxo.txid, utxo.vout));
                    (utxo, asset)
                })
                .filter_map(|(utxo, asset_opt)| match asset_opt {
                    Some(_) => None,
                    None => Some(utxo),
                })
                .map(|utxo| self.utxo_to_json(&utxo, None))
                .collect()
        )))
    }

    fn blockchain_relayfee(&self) -> Result<Value> {
        let relayfee = self.query.get_relayfee()?;
        // convert from tapyrus/b to TPC/kB, as expected by Electrum clients
        Ok(json!(relayfee / 100_000f64))
    }

    fn blockchain_scripthash_subscribe(&mut self, params: &[Value]) -> Result<Value> {
        let script_hash = hash_from_value(params.get(0)).chain_err(|| "bad script_hash")?;

        let history_txids = get_history(&self.query, &script_hash[..], self.txs_limit)?;
        let status_hash = get_status_hash(history_txids, &self.query)
            .map_or(Value::Null, |h| json!(hex::encode(full_hash(&h[..]))));

        if let None = self.status_hashes.insert(script_hash, status_hash.clone()) {
            self.stats.subscriptions.inc();
        }
        Ok(status_hash)
    }

    fn blockchain_scripthash_get_balance(&self, params: &[Value]) -> Result<Value> {
        let script_hash = hash_from_value(params.get(0)).chain_err(|| "bad script_hash")?;
        let (chain_stats, mempool_stats) = self.query.stats(&script_hash[..]);

        let mut color_ids: HashSet<ColorIdentifier> = chain_stats.keys().cloned().collect();
        color_ids.extend(
            mempool_stats
                .keys()
                .cloned()
                .collect::<HashSet<ColorIdentifier>>(),
        );

        Ok(json!(Value::Array(
            color_ids
                .into_iter()
                .map(|key| { self.balance_to_json(&chain_stats, &mempool_stats, key) })
                .collect()
        )))
    }

    fn blockchain_scripthash_get_history(&self, params: &[Value]) -> Result<Value> {
        let script_hash = hash_from_value(params.get(0)).chain_err(|| "bad script_hash")?;
        let history_txids = get_history(&self.query, &script_hash[..], self.txs_limit)?;

        Ok(json!(history_txids
            .into_iter()
            .map(|(txid, blockid)| {
                let is_mempool = blockid.is_none();
                let fee = is_mempool.and_then(|| self.query.get_mempool_tx_fee(&txid));
                let has_unconfirmed_parents = is_mempool
                    .and_then(|| Some(self.query.has_unconfirmed_parents(&txid)))
                    .unwrap_or(false);
                let height = get_electrum_height(blockid, has_unconfirmed_parents);
                GetHistoryResult { txid, height, fee }
            })
            .collect::<Vec<_>>()))
    }

    fn blockchain_scripthash_listunspent(&self, params: &[Value]) -> Result<Value> {
        let script_hash = hash_from_value(params.get(0)).chain_err(|| "bad script_hash")?;
        let utxos = self.query.utxo(&script_hash[..])?;
        Ok(json!(Value::Array(
            utxos
                .into_iter()
                .map(|utxo| self.utxo_to_json(&utxo, None))
                .collect()
        )))
    }

    fn blockchain_scripthash_listcoloredunspent(&self, params: &[Value]) -> Result<Value> {
        let script_hash = hash_from_value(params.get(0)).chain_err(|| "bad script_hash")?;
        let color_id =
            color_id_from_value(params.get(1), "color_id").chain_err(|| "bad color_id")?;
        let utxos = self.query.utxo(&script_hash[..])?;
        Ok(json!(Value::Array(
            utxos
                .into_iter()
                .filter(|o| o.color_id.is_colored()
                    && (color_id.is_none() || color_id == Some(o.color_id.clone())))
                .map(|utxo| self.utxo_to_json(&utxo, None))
                .collect()
        )))
    }

    fn blockchain_scripthash_listuncoloredunspent(&self, params: &[Value]) -> Result<Value> {
        let script_hash = hash_from_value(params.get(0)).chain_err(|| "bad script_hash")?;
        let utxos = self.query.utxo(&script_hash[..])?;
        Ok(json!(Value::Array(
            utxos
                .into_iter()
                .filter(|o| o.color_id.is_default())
                .map(|utxo| self.utxo_to_json(&utxo, None))
                .collect()
        )))
    }

    fn blockchain_transaction_broadcast(&self, params: &[Value]) -> Result<Value> {
        let tx = params.get(0).chain_err(|| "missing tx")?;
        let tx = tx.as_str().chain_err(|| "non-string tx")?.to_string();
        let txid = self.query.broadcast_raw(&tx)?;
        if let Err(e) = self.chan.sender().try_send(Message::PeriodicUpdate) {
            warn!("failed to issue PeriodicUpdate after broadcast: {}", e);
        }
        Ok(json!(txid))
    }

    fn blockchain_transaction_get(&self, params: &[Value]) -> Result<Value> {
        let tx_hash = Txid::from(hash_from_value(params.get(0)).chain_err(|| "bad tx_hash")?);
        let verbose = match params.get(1) {
            Some(value) => value.as_bool().chain_err(|| "non-bool verbose value")?,
            None => false,
        };

        let tx = self
            .query
            .lookup_raw_txn(&tx_hash)
            .chain_err(|| "missing transaction")?;
        if verbose {
            let tx: Transaction = deserialize(&tx).chain_err(|| "can not deserialize raw tx")?;
            let blockid = self
                .query
                .chain()
                .tx_confirming_block(&tx_hash)
                .ok_or_else(|| "tx not found or is unconfirmed")?;
            let entry = self
                .query
                .chain()
                .header_by_height(blockid.height)
                .ok_or_else(|| "block not found")?;
            let best_height = self
                .query
                .chain()
                .best_height();
            let confirmations = best_height - entry.height();
            Ok(self.transaction_to_json(&tx, confirmations, &entry.header()))
        } else {
            Ok(json!(hex::encode(tx)))
        }
    }

    fn blockchain_transaction_get_merkle(&self, params: &[Value]) -> Result<Value> {
        let txid = Txid::from(hash_from_value(params.get(0)).chain_err(|| "bad tx_hash")?);
        let height = usize_from_value(params.get(1), "height")?;
        let blockid = self
            .query
            .chain()
            .tx_confirming_block(&txid)
            .ok_or_else(|| "tx not found or is unconfirmed")?;
        if blockid.height != height {
            bail!("invalid confirmation height provided");
        }
        let (merkle, pos) = get_tx_merkle_proof(self.query.chain(), &txid, &blockid.hash)
            .chain_err(|| "cannot create merkle proof")?;
        Ok(json!({
                "block_height": blockid.height,
                "merkle": merkle,
                "pos": pos}))
    }

    fn blockchain_transaction_id_from_pos(&self, params: &[Value]) -> Result<Value> {
        let height = usize_from_value(params.get(0), "height")?;
        let tx_pos = usize_from_value(params.get(1), "tx_pos")?;
        let want_merkle = bool_from_value_or(params.get(2), "merkle", false)?;

        let (txid, merkle) = get_id_from_pos(self.query.chain(), height, tx_pos, want_merkle)?;

        if !want_merkle {
            return Ok(json!(txid));
        }

        Ok(json!({
            "tx_hash": txid,
            "merkle" : merkle}))
    }

    fn handle_command(&mut self, method: &str, params: &[Value], id: &Value) -> Result<Value> {
        let timer = self
            .stats
            .latency
            .with_label_values(&[method])
            .start_timer();
        let result = match method {
            "blockchain.block.header" => self.blockchain_block_header(&params),
            "blockchain.block.headers" => self.blockchain_block_headers(&params),
            "blockchain.estimatefee" => self.blockchain_estimatefee(&params),
            "blockchain.headers.subscribe" => self.blockchain_headers_subscribe(),
            "blockchain.openassets.scripthash.listunspent" if self.enable_open_assets => {
                self.blockchain_openassets_scripthash_listunspent(&params)
            }
            "blockchain.openassets.scripthash.listcoloredunspent" if self.enable_open_assets => {
                self.blockchain_openassets_scripthash_listcoloredunspent(&params)
            }
            "blockchain.openassets.scripthash.listuncoloredunspent" if self.enable_open_assets => {
                self.blockchain_openassets_scripthash_listuncoloredunspent(&params)
            }
            "blockchain.relayfee" => self.blockchain_relayfee(),
            "blockchain.scripthash.get_balance" => self.blockchain_scripthash_get_balance(&params),
            "blockchain.scripthash.get_history" => self.blockchain_scripthash_get_history(&params),
            "blockchain.scripthash.listunspent" => self.blockchain_scripthash_listunspent(&params),
            "blockchain.scripthash.listcoloredunspent" => {
                self.blockchain_scripthash_listcoloredunspent(&params)
            }
            "blockchain.scripthash.listuncoloredunspent" => {
                self.blockchain_scripthash_listuncoloredunspent(&params)
            }
            "blockchain.scripthash.subscribe" => self.blockchain_scripthash_subscribe(&params),
            "blockchain.transaction.broadcast" => self.blockchain_transaction_broadcast(&params),
            "blockchain.transaction.get" => self.blockchain_transaction_get(&params),
            "blockchain.transaction.get_merkle" => self.blockchain_transaction_get_merkle(&params),
            "blockchain.transaction.id_from_pos" => {
                self.blockchain_transaction_id_from_pos(&params)
            }
            "mempool.get_fee_histogram" => self.mempool_get_fee_histogram(),
            "server.banner" => self.server_banner(),
            "server.donation_address" => self.server_donation_address(),
            "server.peers.subscribe" => self.server_peers_subscribe(),
            "server.ping" => Ok(Value::Null),
            "server.version" => self.server_version(),

            &_ => bail!("unknown method {} {:?}", method, params),
        };
        timer.observe_duration();
        // TODO: return application errors should be sent to the client
        Ok(match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(e) => {
                warn!(
                    "rpc #{} {} {:?} failed: {}",
                    id,
                    method,
                    params,
                    e.display_chain()
                );
                json!({"jsonrpc": "2.0", "id": id, "error": format!("{}", e)})
            }
        })
    }

    fn update_subscriptions(&mut self) -> Result<Vec<Value>> {
        let timer = self
            .stats
            .latency
            .with_label_values(&["periodic_update"])
            .start_timer();
        let mut result = vec![];
        if let Some(ref mut last_entry) = self.last_header_entry {
            let entry = self.query.chain().best_header();
            if *last_entry != entry {
                *last_entry = entry;
                let hex_header = hex::encode(serialize(last_entry.header()));
                let header = json!({"hex": hex_header, "height": last_entry.height()});
                result.push(json!({
                    "jsonrpc": "2.0",
                    "method": "blockchain.headers.subscribe",
                    "params": [header]}));
            }
        }
        for (script_hash, status_hash) in self.status_hashes.iter_mut() {
            let history_txids = get_history(&self.query, &script_hash[..], self.txs_limit)?;
            let new_status_hash = get_status_hash(history_txids, &self.query)
                .map_or(Value::Null, |h| json!(hex::encode(full_hash(&h[..]))));
            if new_status_hash == *status_hash {
                continue;
            }
            result.push(json!({
                "jsonrpc": "2.0",
                "method": "blockchain.scripthash.subscribe",
                "params": [script_hash, new_status_hash]}));
            *status_hash = new_status_hash;
        }
        timer.observe_duration();
        Ok(result)
    }

    fn send_values(&mut self, values: &[Value]) -> Result<()> {
        for value in values {
            let line = value.to_string() + "\n";
            self.stream
                .write_all(line.as_bytes())
                .chain_err(|| format!("failed to send {}", value))?;
        }
        Ok(())
    }

    fn handle_replies(&mut self) -> Result<()> {
        let empty_params = json!([]);
        loop {
            let msg = self.chan.receiver().recv().chain_err(|| "channel closed")?;
            trace!("RPC {:?}", msg);
            match msg {
                Message::Request(line) => {
                    let cmd: Value = from_str(&line).chain_err(|| "invalid JSON format")?;
                    let reply = match (
                        cmd.get("method"),
                        cmd.get("params").unwrap_or_else(|| &empty_params),
                        cmd.get("id"),
                    ) {
                        (
                            Some(&Value::String(ref method)),
                            &Value::Array(ref params),
                            Some(ref id),
                        ) => self.handle_command(method, params, id)?,
                        _ => bail!("invalid command: {}", cmd),
                    };
                    self.send_values(&[reply])?
                }
                Message::PeriodicUpdate => {
                    let values = self
                        .update_subscriptions()
                        .chain_err(|| "failed to update subscriptions")?;
                    self.send_values(&values)?
                }
                Message::Done => return Ok(()),
            }
        }
    }

    fn handle_requests(mut reader: BufReader<TcpStream>, tx: SyncSender<Message>) -> Result<()> {
        loop {
            let mut line = Vec::<u8>::new();
            reader
                .read_until(b'\n', &mut line)
                .chain_err(|| "failed to read a request")?;
            if line.is_empty() {
                tx.send(Message::Done).chain_err(|| "channel closed")?;
                return Ok(());
            } else {
                if line.starts_with(&[22, 3, 1]) {
                    // (very) naive SSL handshake detection
                    let _ = tx.send(Message::Done);
                    bail!("invalid request - maybe SSL-encrypted data?: {:?}", line)
                }
                match String::from_utf8(line) {
                    Ok(req) => tx
                        .send(Message::Request(req))
                        .chain_err(|| "channel closed")?,
                    Err(err) => {
                        let _ = tx.send(Message::Done);
                        bail!("invalid UTF8: {}", err)
                    }
                }
            }
        }
    }

    pub fn run(mut self) {
        self.stats.clients.inc();
        let reader = BufReader::new(self.stream.try_clone().expect("failed to clone TcpStream"));
        let tx = self.chan.sender();
        let child = spawn_thread("reader", || Connection::handle_requests(reader, tx));
        if let Err(e) = self.handle_replies() {
            error!(
                "[{}] connection handling failed: {}",
                self.addr,
                e.display_chain().to_string()
            );
        }
        self.stats.clients.dec();
        self.stats
            .subscriptions
            .sub(self.status_hashes.len() as i64);

        debug!("[{}] shutting down connection", self.addr);
        let _ = self.stream.shutdown(Shutdown::Both);
        if let Err(err) = child.join().expect("receiver panicked") {
            error!("[{}] receiver failed: {}", self.addr, err);
        }
    }
}

fn get_history(
    query: &Query,
    scripthash: &[u8],
    txs_limit: usize,
) -> Result<Vec<(Txid, Option<BlockId>)>> {
    // to avoid silently trunacting history entries, ask for one extra more than the limit and fail if it exists
    let history_txids = query.history_txids(scripthash, txs_limit + 1);
    ensure!(history_txids.len() <= txs_limit, ErrorKind::TooPopular);
    Ok(history_txids)
}

#[derive(Serialize, Debug)]
struct GetHistoryResult {
    #[serde(rename = "tx_hash")]
    txid: Txid,
    height: isize,
    #[serde(skip_serializing_if = "Option::is_none")]
    fee: Option<u64>,
}

#[derive(Debug)]
pub enum Message {
    Request(String),
    PeriodicUpdate,
    Done,
}

pub enum Notification {
    Periodic,
    Exit,
}

pub struct RPC {
    notification: Sender<Notification>,
    server: Option<thread::JoinHandle<()>>, // so we can join the server while dropping this ojbect
}

struct Stats {
    latency: HistogramVec,
    clients: Gauge,
    subscriptions: Gauge,
}

impl RPC {
    fn start_notifier(
        notification: Channel<Notification>,
        senders: Arc<Mutex<Vec<SyncSender<Message>>>>,
        acceptor: Sender<Option<(TcpStream, SocketAddr)>>,
    ) {
        spawn_thread("notification", move || {
            for msg in notification.receiver().iter() {
                let mut senders = senders.lock().unwrap();
                match msg {
                    Notification::Periodic => {
                        for sender in senders.split_off(0) {
                            if let Err(TrySendError::Disconnected(_)) =
                                sender.try_send(Message::PeriodicUpdate)
                            {
                                continue;
                            }
                            senders.push(sender);
                        }
                    }
                    Notification::Exit => acceptor.send(None).unwrap(), // mark acceptor as done
                }
            }
        });
    }

    fn start_acceptor(addr: SocketAddr) -> Channel<Option<(TcpStream, SocketAddr)>> {
        let chan = Channel::unbounded();
        let acceptor = chan.sender();
        spawn_thread("acceptor", move || {
            let socket = create_socket(&addr);
            socket.listen(511).expect("setting backlog failed");
            socket
                .set_nonblocking(false)
                .expect("cannot set nonblocking to false");
            let listener = socket.into_tcp_listener();

            info!("Electrum RPC server running on {}", addr);
            loop {
                let (stream, addr) = listener.accept().expect("accept failed");
                stream
                    .set_nonblocking(false)
                    .expect("failed to set connection as blocking");
                acceptor.send(Some((stream, addr))).expect("send failed");
            }
        });
        chan
    }

    pub fn start(config: Arc<Config>, query: Arc<Query>, metrics: &Metrics) -> RPC {
        let stats = Arc::new(Stats {
            latency: metrics.histogram_vec(
                HistogramOpts::new("electrum_rpc", "Electrum RPC latency (seconds)"),
                &["method"],
            ),
            clients: metrics.gauge(MetricOpts::new("electrum_clients", "# of Electrum clients")),
            subscriptions: metrics.gauge(MetricOpts::new(
                "electrum_subscriptions",
                "# of Electrum subscriptions",
            )),
        });
        stats.clients.set(0);
        stats.subscriptions.set(0);

        let notification = Channel::unbounded();
        let rpc_addr = config.electrum_rpc_addr;
        let txs_limit = config.electrum_txs_limit;
        let enable_open_assets = config.enable_open_assets;

        RPC {
            notification: notification.sender(),
            server: Some(spawn_thread("rpc", move || {
                let senders = Arc::new(Mutex::new(Vec::<SyncSender<Message>>::new()));

                let acceptor = RPC::start_acceptor(rpc_addr);
                RPC::start_notifier(notification, senders.clone(), acceptor.sender());

                let mut threads = HashMap::new();
                let (garbage_sender, garbage_receiver) = crossbeam_channel::unbounded();

                while let Some((stream, addr)) = acceptor.receiver().recv().unwrap() {
                    // explicitely scope the shadowed variables for the new thread
                    let query = Arc::clone(&query);
                    let senders = Arc::clone(&senders);
                    let stats = Arc::clone(&stats);
                    let garbage_sender = garbage_sender.clone();
                    let spawned = spawn_thread("peer", move || {
                        info!("[{}] connected peer", addr);
                        let conn = Connection::new(
                            query,
                            stream,
                            addr,
                            stats,
                            txs_limit,
                            enable_open_assets,
                        );
                        senders.lock().unwrap().push(conn.chan.sender());
                        conn.run();
                        info!("[{}] disconnected peer", addr);
                        let _ = garbage_sender.send(std::thread::current().id());
                    });

                    trace!("[{}] spawned {:?}", addr, spawned.thread().id());
                    threads.insert(spawned.thread().id(), spawned);
                    while let Ok(id) = garbage_receiver.try_recv() {
                        if let Some(thread) = threads.remove(&id) {
                            trace!("[{}] joining {:?}", addr, id);
                            if let Err(error) = thread.join() {
                                error!("failed to join {:?}: {:?}", id, error);
                            }
                        }
                    }
                }

                trace!("closing {} RPC connections", senders.lock().unwrap().len());
                for sender in senders.lock().unwrap().iter() {
                    let _ = sender.send(Message::Done);
                }

                for (id, thread) in threads {
                    trace!("joining {:?}", id);
                    if let Err(error) = thread.join() {
                        error!("failed to join {:?}: {:?}", id, error);
                    }
                }

                trace!("RPC connections are closed");
            })),
        }
    }

    pub fn notify(&self) {
        self.notification.send(Notification::Periodic).unwrap();
    }
}

impl Drop for RPC {
    fn drop(&mut self) {
        trace!("stop accepting new RPCs");
        self.notification.send(Notification::Exit).unwrap();
        if let Some(handle) = self.server.take() {
            handle.join().unwrap();
        }
        trace!("RPC server is stopped");
    }
}
