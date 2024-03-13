# Esplora HTTP API

JSON over RESTful HTTP. Amounts are always represented in tapyrus.

You can also [self-host the Esplora API server](https://github.com/Blockstream/esplora#how-to-run-the-explorer-for-bitcoin-mainnet), which provides better privacy and security.

## Transactions

### `GET /tx/:txid`

Returns information about the transaction.

Available fields: `txid`, `version`, `locktime`, `size`, `weight`, `fee`, `vin`, `vout` and `status`
(see [transaction format](#transaction-format) for details).

### `GET /tx/:txid/status`

Returns the transaction confirmation status.

Available fields: `confirmed` (boolean), `block_height` (optional) and `block_hash` (optional).

### `GET /tx/:txid/hex`
### `GET /tx/:txid/raw`

Returns the raw transaction in hex or as binary data.

### `GET /tx/:txid/merkleblock-proof`

Returns a merkle inclusion proof for the transaction using
[tapyrusd's merkleblock](https://bitcoin.org/en/glossary/merkle-block) format.

### `GET /tx/:txid/merkle-proof`

Returns a merkle inclusion proof for the transaction using
[Electrum's `blockchain.transaction.get_merkle`](https://electrumx.readthedocs.io/en/latest/protocol-methods.html#blockchain-transaction-get-merkle)
format.

### `GET /tx/:txid/outspend/:vout`

Returns the spending status of a transaction output.

Available fields: `spent` (boolean), `txid` (optional), `vin` (optional) and `status` (optional, the status of the spending tx).

### `GET /tx/:txid/outspends`

Returns the spending status of all transaction outputs.

### `POST /tx`

Broadcast a raw transaction to the network.

The transaction should be provided as hex in the request body.
The `txid` will be returned on success.

## Addresses

### `GET /address/:address`
### `GET /scripthash/:hash`

Get information about an address/scripthash.

Available fields: `address`/`scripthash`, `chain_stats` and `mempool_stats`.

`{chain,mempool}_stats` each contain an object with `tx_count`, `funded_txo_count`, `funded_txo_sum`, `spent_txo_count` and `spent_txo_sum`.

### `GET /address/:address/txs`
### `GET /scripthash/:hash/txs`

Get transaction history for the specified address/scripthash, sorted with newest first.

Returns up to 50 mempool transactions plus the first 25 confirmed transactions.
You can request more confirmed transactions using `:last_seen_txid`(see below).

### `GET /address/:address/txs/chain[/:last_seen_txid]`
### `GET /scripthash/:hash/txs/chain[/:last_seen_txid]`

Get confirmed transaction history for the specified address/scripthash, sorted with newest first.

Returns 25 transactions per page. More can be requested by specifying the last txid seen by the previous query.

### `GET /address/:address/txs/mempool`
### `GET /scripthash/:hash/txs/mempool`

Get unconfirmed transaction history for the specified address/scripthash.

Returns up to 50 transactions (no paging).

### `GET /address/:address/utxo`
### `GET /scripthash/:hash/utxo`

Get the list of unspent transaction outputs associated with the address/scripthash.

Available fields: `txid`, `vout`, `value` and `status` (with the status of the funding tx).

### `GET /address-prefix/:prefix`

Search for addresses beginning with `:prefix`.

Returns a JSON array with up to 10 results.

## Blocks

### `GET /block/:hash`

Returns information about a block.

Available fields: `id`, `height`, `features`, `time`, `merkle_root`, `im_merkle_root`, `tx_count`, `size`, `weight`, `previousblockhash` `xfield_type`, `xfield` and `signature`.
See [block format](#block-format) for more details.

The response from this endpoint can be cached indefinitely.

### `GET /block/:hash/header`

Returns the hex-encoded block header.

The response from this endpoint can be cached indefinitely.

### `GET /block/:hash/status`

Returns the block status.

Available fields: `in_best_chain` (boolean, false for orphaned blocks), `next_best` (the hash of the next block, only available for blocks in the best chain).

### `GET /block/:hash/txs[/:start_index]`

Returns a list of transactions in the block (up to 25 transactions beginning at `start_index`).

Transactions returned here do not have the `status` field, since all the transactions share the same block and confirmation status.

The response from this endpoint can be cached indefinitely.

### `GET /block/:hash/txids`

Returns a list of all txids in the block.

The response from this endpoint can be cached indefinitely.

### `GET /block/:hash/txid/:index`

Returns the transaction at index `:index` within the specified block.

The response from this endpoint can be cached indefinitely.

### `GET /block/:hash/raw`

Returns the raw block representation in binary.

The response from this endpoint can be cached indefinitely.

### `GET /block-height/:height`

Returns the hash of the block currently at `height`.

### `GET /blocks[/:start_height]`

Returns the 10 newest blocks starting at the tip or at `start_height` if specified.

### `GET /blocks/tip/height`

Returns the height of the last block.

### `GET /blocks/tip/hash`

Returns the hash of the last block.

## Mempool

### `GET /mempool`

Get mempool backlog statistics. Returns an object with:

- `count`: the number of transactions in the mempool

- `vsize`: the total size of mempool transactions in virtual bytes

- `total_fee`: the total fee paid by mempool transactions in satoshis

- `fee_histogram`: mempool fee-rate distribution histogram

  An array of `(feerate, vsize)` tuples, where each entry's `vsize` is the total vsize of transactions
  paying more than `feerate` but less than the previous entry's `feerate` (except for the first entry, which has no upper bound).
  This matches the format used by the Electrum RPC protocol for `mempool.get_fee_histogram`.

Example output:

```
{
  "count": 8134,
  "vsize": 3444604,
  "total_fee":29204625,
  "fee_histogram": [[53.01, 102131], [38.56, 110990], [34.12, 138976], [24.34, 112619], [3.16, 246346], [2.92, 239701], [1.1, 775272]]
}
```

> In this example, there are transactions weighting a total of 102,131 vbytes that are paying more than 53 sat/vB,
110,990 vbytes of transactions paying between 38 and 53 sat/vB, 138,976 vbytes paying between 34 and 38, etc.


### `GET /mempool/txids`

Get the full list of txids in the mempool as an array.

The order of the txids is arbitrary and does not match tapyrusd's.

### `GET /mempool/recent`

Get a list of the last 10 transactions to enter the mempool.

Each transaction object contains simplified overview data, with the following fields: `txid`, `fee`, `vsize`, `time` and `value`

### `GET /mempool/txs[/:start_index]`

Get a list of the transactions to enter the mempool (up to 25 transactions beginning at `start_index`).

Each transaction object contains simplified overview data, with the following fields: `txid`, `fee`, `vsize`, `time` and `value`

## Fee estimates

### `GET /fee-estimates`

Get an object where the key is the confirmation target (in number of blocks)
and the value is the estimated feerate (in sat/vB).

The available confirmation targets are 1-25, 144, 504 and 1008 blocks.

For example: `{ "1": 87.882, "2": 87.882, "3": 87.882, "4": 87.882, "5": 81.129, "6": 68.285, ..., "144": 1.027, "504": 1.027, "1008": 1.027 }`

## Tokens

### `GET /colors[/:last_seen_color_id]`

Returns a list of the 25 `color_id`s

if `last_seen_color_id` specified, returns a list of the next 25 `color_id`s after specified `last_seen_color_id`


### `GET /color/:color_id`

Get information about a colored coin.

Returns an object with:

- `color_id`
- `chain_stats` and `mempool_stats`, each with:
  - `tx_count`
  - `issued_tx_count`
  - `transferred_tx_count`
  - `burned_tx_count`
  - `issued_sum`
  - `transferred_sum`
  - `burned_sum`

Example :

```
{
  "color_id": "c16f0279e9ed041c3d710a9f57d0c02928416460c4b722ae3457a11eec381c526d",
  "chain_stats": {
    "tx_count": 54,
    "issued_tx_count": 2,
    "issued_sum": 1600000000,
    "transferred_tx_count": 1,
    "transferred_sum": 600000000,
    "burned_tx_count": 0,
    "burned_sum": 0
  },
  "mempool_stats": {
    "tx_count": 3,
    "issued_tx_count": 0,
    "issued_sum": 0,
    "transferred_tx_count": 0,
    "transferred_sum": 0,
    "burned_tx_count": 1,
    "burned_sum": 100000000
  }
}
```

### `GET /color/:color_id/txs`
### `GET /color/:color_id/txs/mempool`
### `GET /color/:color_id/txs/chain[/:last_seen]`

Get transactions associated with the specified color coin.

Returns a list of issuance, reissuance, transfer and burn transactions.

The total number of results will be returned as the `x-total-results` header.

## Transaction format

- `txid`
- `version`
- `locktime`
- `size`
- `weight`
- `fee`
- `vin[]`
  - `txid`
  - `vout`
  - `is_coinbase`
  - `scriptsig`
  - `scriptsig_asm`
  - `inner_redeemscript_asm`
  - `sequence`
  - `prevout` (previous output in the same format as in `vout` below)
- `vout[]`
  - `scriptpubkey`
  - `scriptpubkey_asm`
  - `scriptpubkey_type`
  - `scriptpubkey_address`
  - `value`
- `status`
  - `confirmed` (boolean)
  - `block_height` (available for confirmed transactions, `null` otherwise)
  - `block_hash` (available for confirmed transactions, `null` otherwise)
  - `block_time` (available for confirmed transactions, `null` otherwise)

## Block format

- `id`
- `height`
- `features`
- `time`
- `tx_count`
- `size`
- `weight`
- `merkle_root`
- `im_merkle_root`
- `previousblockhash`
- `signature`
- `xfield_type`
- `xfield`


