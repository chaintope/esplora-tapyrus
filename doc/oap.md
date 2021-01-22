# Support for open assets protocol

The Open Assets Protocol is a simple and powerful protocol. 
It allows issuance and transfer of user-created assets.

The Open Assets Protocol is designed for Bitcoin blockchain, but it is possible to adapt to Tapyrus blockchain.

To support it in electrs, some API should be updated or added.

## Updated API

### blockchain.scripthash.get_balance

Return the confirmed and unconfirmed balances of a script hash.

#### Signature

blockchain.scripthash.get_balance(scripthash)

scripthash

The script hash as a hexadecimal string.

#### Result

A dictionary with keys confirmed, unconfirmed and assets. The value of confirmed and unconfirmed is the appropriate balance in coin units as a string.

#### Result Example

```
{
  "confirmed": "1.03873966",
  "unconfirmed": "0.236844"
  "assets": [
    {
      "asset_type": "4F41",
      "asset_id": "36e0ea8e93eaa0285d641305f4c81e563aa570a2",
      "asset_quantity": 100
    },
    {
      "asset_type": "4F41",
      "asset_id": "f8cc4f23cdedeca2c429aef5b3345e666c411ece",
      "asset_quantity": 200
    }
  ]
}
```

### blockchain.scripthash.listunspent

Return an ordered list of UTXOs sent to a script hash.

#### Signature

blockchain.scripthash.listunspent(scripthash)

  scripthash

The script hash as a hexadecimal string.

#### Result

A list of unspent outputs in blockchain order. This function takes the mempool into account. Mempool transactions paying to the address are included at the end of the list in an undefined order. Any output that is spent in the mempool does not appear. Each output is a dictionary with the following keys:

- height

    The integer height of the block the transaction was confirmed in. 0 if the transaction is in the mempool.

- tx_pos

    The zero-based index of the output in the transaction’s list of outputs.

- tx_hash

    The output’s transaction hash as a hexadecimal string.

- value

    The output’s value in minimum coin units (satoshis).

- asset_type

    The asset type as a hexadecimal string. "4F41" for the Open Assets. null if the utxo is uncolored.

- asset_id

    The asset id as a hexadecimal string. This is a 160 bits hash for the Open Assets. null if the utxo is uncolored.

- asset_quantity

    The asset quantity of the output as an unsigned integer. 0 if the utxo is uncolored.


#### Result Example

```
[
  {
    "tx_pos": 0,
    "value": 45318048,
    "tx_hash": "9f2c45a12db0144909b5db269415f7319179105982ac70ed80d76ea79d923ebf",
    "height": 437146
    "asset_type": "4F41",
    "asset_id": "36e0ea8e93eaa0285d641305f4c81e563aa570a2",
    "asset_quantity": 10
  },
  {
    "tx_pos": 0,
    "value": 919195,
    "tx_hash": "3d2290c93436a3e964cfc2f0950174d8847b1fbe3946432c4784e168da0f019f",
    "height": 441696
    "asset_type": null,
    "asset_id": null,
    "asset_quantity": 0
  }
]
```

### blockchain.transaction.get

Return a raw transaction.

#### Signature

blockchain.transaction.get(tx_hash, verbose=false, merkle=false)

tx_hash

The transaction hash as a hexadecimal string.
verbose

Whether a verbose coin-specific response is required.

#### Result

If verbose is false:

The raw transaction as a hexadecimal string.

If verbose is true:

The result is a coin-specific dictionary – whatever the coin daemon returns when asked for a verbose form of the raw transaction.

#### Example Results

When verbose is false:

```
"01000000015bb9142c960a838329694d3fe9ba08c2a6421c5158d8f7044cb7c48006c1b48"
"4000000006a4730440220229ea5359a63c2b83a713fcc20d8c41b20d48fe639a639d2a824"
"6a137f29d0fc02201de12de9c056912a4e581a62d12fb5f43ee6c08ed0238c32a1ee76921"
"3ca8b8b412103bcf9a004f1f7a9a8d8acce7b51c983233d107329ff7c4fb53e44c855dbe1"
"f6a4feffffff02c6b68200000000001976a9141041fb024bd7a1338ef1959026bbba86006"
"4fe5f88ac50a8cf00000000001976a91445dac110239a7a3814535c15858b939211f85298"
"88ac61ee0700"
```

When verbose is true:

```
{
  "blockhash": "0000000000000000015a4f37ece911e5e3549f988e855548ce7494a0a08b2ad6",
  "blocktime": 1520074861,
  "confirmations": 679,
  "hash": "36a3692a41a8ac60b73f7f41ee23f5c917413e5b2fad9e44b34865bd0d601a3d",
  "hex": "01000000015bb9142c960a838329694d3fe9ba08c2a6421c5158d8f7044cb7c48006c1b484000000006a4730440220229ea5359a63c2b83a713fcc20d8c41b20d48fe639a639d2a8246a137f29d0fc02201de12de9c056912a4e581a62d12fb5f43ee6c08ed0238c32a1ee769213ca8b8b412103bcf9a004f1f7a9a8d8acce7b51c983233d107329ff7c4fb53e44c855dbe1f6a4feffffff02c6b68200000000001976a9141041fb024bd7a1338ef1959026bbba860064fe5f88ac50a8cf00000000001976a91445dac110239a7a3814535c15858b939211f8529888ac61ee0700",
  "locktime": 519777,
  "size": 225,
  "time": 1520074861,
  "txid": "36a3692a41a8ac60b73f7f41ee23f5c917413e5b2fad9e44b34865bd0d601a3d",
  "version": 1,
  "vin": [ {
    "scriptSig": {
      "asm": "30440220229ea5359a63c2b83a713fcc20d8c41b20d48fe639a639d2a8246a137f29d0fc02201de12de9c056912a4e581a62d12fb5f43ee6c08ed0238c32a1ee769213ca8b8b[ALL|FORKID] 03bcf9a004f1f7a9a8d8acce7b51c983233d107329ff7c4fb53e44c855dbe1f6a4",
      "hex": "4730440220229ea5359a63c2b83a713fcc20d8c41b20d48fe639a639d2a8246a137f29d0fc02201de12de9c056912a4e581a62d12fb5f43ee6c08ed0238c32a1ee769213ca8b8b412103bcf9a004f1f7a9a8d8acce7b51c983233d107329ff7c4fb53e44c855dbe1f6a4"
    },
    "sequence": 4294967294,
    "txid": "84b4c10680c4b74c04f7d858511c42a6c208bae93f4d692983830a962c14b95b",
    "vout": 0}],
  "vout": [ { "n": 0,
             "scriptPubKey": { "addresses": [ "12UxrUZ6tyTLoR1rT1N4nuCgS9DDURTJgP"],
                               "asm": "OP_DUP OP_HASH160 1041fb024bd7a1338ef1959026bbba860064fe5f OP_EQUALVERIFY OP_CHECKSIG",
                               "hex": "76a9141041fb024bd7a1338ef1959026bbba860064fe5f88ac",
                               "reqSigs": 1,
                               "type": "pubkeyhash"},
             "value": 0.0856647},
           { "n": 1,
             "scriptPubKey": { "addresses": [ "17NMgYPrguizvpJmB1Sz62ZHeeFydBYbZJ"],
                               "asm": "OP_DUP OP_HASH160 45dac110239a7a3814535c15858b939211f85298 OP_EQUALVERIFY OP_CHECKSIG",
                               "hex": "76a91445dac110239a7a3814535c15858b939211f8529888ac",
                               "reqSigs": 1,
                               "type": "pubkeyhash"},
             "value": 0.1360904,
             "asset_type": "4F41",
             "asset_id": "36e0ea8e93eaa0285d641305f4c81e563aa570a2",
             "asset_quantity": 100
            }]}
```

## New API

### blockchain.scripthash.listcoloredunspent

Return an ordered list of colored UTXOs sent to a scripthash.

#### Signature
  blockchain.scripthash.listcoloredunspent(scripthash)

  scripthash: The script hash as a hexadecimal string.

#### Result
  A list of colored unspent outputs in blockchain order.
  Each output is a dictionary with the following keys:

- height

  The integer height of the block the transaction was confirmed in. 0 if the transaction is in the mempool.

- tx_pos

  The zero-based index of the output in the transaction’s list of outputs.

- tx_hash

  The output’s transaction hash as a hexadecimal string.

- value

  The output’s value in minimum coin units (satoshis).

- asset_type

  The asset type as a hexadecimal string. "4F41" for the Open Assets.

- asset_id

  The asset id as a hexadecimal string. This is a 160 bits hash for the Open Assets

- asset_quantity

  The asset quantity of the output as an unsigned integer.

#### Result Example

```
[
  {
    "tx_pos": 0,
    "value": 45318048,
    "tx_hash": "9f2c45a12db0144909b5db269415f7319179105982ac70ed80d76ea79d923ebf",
    "height": 437146
    "asset_type": "4F41",
    "asset_id": "36e0ea8e93eaa0285d641305f4c81e563aa570a2",
    "asset_quantity": 100
  },
  {
    "tx_pos": 0,
    "value": 919195,
    "tx_hash": "3d2290c93436a3e964cfc2f0950174d8847b1fbe3946432c4784e168da0f019f",
    "height": 441696,
    "asset_type": "4F41",
    "asset_id": "36e0ea8e93eaa0285d641305f4c81e563aa570a2",
    "asset_quantity": 10
  }
]
```

### blockchain.scripthash.listuncoloredunspent

Return an ordered list of uncolored UTXOs sent to a scripthash.

#### Signature
  blockchain.scripthash.listuncoloredunspent(scripthash)

  scripthash: The script hash as a hexadecimal string.

#### Result
  A list of uncolored unspent outputs in blockchain order.
  Each output is a dictionary with the following keys:

- height

  The integer height of the block the transaction was confirmed in. 0 if the transaction is in the mempool.

- tx_pos

  The zero-based index of the output in the transaction’s list of outputs.

- tx_hash

  The output’s transaction hash as a hexadecimal string.

- value

  The output’s value in minimum coin units (satoshis).

#### Result Example

```
[
  {
    "tx_pos": 0,
    "value": 45318048,
    "tx_hash": "9f2c45a12db0144909b5db269415f7319179105982ac70ed80d76ea79d923ebf",
    "height": 437146
  },
  {
    "tx_pos": 0,
    "value": 919195,
    "tx_hash": "3d2290c93436a3e964cfc2f0950174d8847b1fbe3946432c4784e168da0f019f",
    "height": 441696
  }
]
```