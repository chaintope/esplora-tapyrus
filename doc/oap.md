# Support for open assets protocol

The Open Assets Protocol is a simple and powerful protocol. 
It allows issuance and transfer of user-created assets.

The Open Assets Protocol is designed for Bitcoin blockchain, but it is possible to adapt to Tapyrus blockchain.

To support it in esplora-tapyrus, some API should be added.

## blockchain.openassets.scripthash.listunspent

Return an ordered list of UTXOs sent to a script hash.

### Signature

blockchain.openassets.scripthash.listunspent(scripthash)

  scripthash

The script hash as a hexadecimal string.

### Result

A list of unspent outputs in blockchain order. This function takes the mempool into account. Mempool transactions paying to the address are included at the end of the list in an undefined order. Any output that is spent in the mempool does not appear. Each output is a dictionary with the following keys:

- height

    The integer height of the block the transaction was confirmed in. 0 if the transaction is in the mempool.

- tx_pos

    The zero-based index of the output in the transaction’s list of outputs.

- tx_hash

    The output’s transaction hash as a hexadecimal string.

- value

    The output’s value in minimum coin units (satoshis).

- asset_id

    The asset id as a hexadecimal string. This is a 160 bits hash for the Open Assets. null if the utxo is uncolored.

- asset_quantity

    The asset quantity of the output as an unsigned integer. 0 if the utxo is uncolored.


### Result Example

```
[
  {
    "tx_pos": 0,
    "value": 45318048,
    "tx_hash": "9f2c45a12db0144909b5db269415f7319179105982ac70ed80d76ea79d923ebf",
    "height": 437146,
    "asset": {
      "asset_id": "ALn3aK1fSuG27N96UGYB1kUYUpGKRhBuBC",
      "asset_quantity": 10
    }
  },
  {
    "tx_pos": 0,
    "value": 919195,
    "tx_hash": "3d2290c93436a3e964cfc2f0950174d8847b1fbe3946432c4784e168da0f019f",
    "height": 441696
  }
]
```

## blockchain.openassets.scripthash.listcoloredunspent

Return an ordered list of colored UTXOs sent to a scripthash.

### Signature
  blockchain.openassets.scripthash.listcoloredunspent(scripthash)

  scripthash: The script hash as a hexadecimal string.

### Result
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

- asset_id

  The asset id as a hexadecimal string. This is a 160 bits hash for the Open Assets

- asset_quantity

  The asset quantity of the output as an unsigned integer.

### Result Example

```
[
  {
    "tx_pos": 0,
    "value": 45318048,
    "tx_hash": "9f2c45a12db0144909b5db269415f7319179105982ac70ed80d76ea79d923ebf",
    "height": 437146
    "asset": {
      "asset_id": "ALVYPzjZjSLoT4xAHJ7x38RcHWNatP4Mba",
      "asset_quantity": 100
    }
  },
  {
    "tx_pos": 0,
    "value": 919195,
    "tx_hash": "3d2290c93436a3e964cfc2f0950174d8847b1fbe3946432c4784e168da0f019f",
    "height": 441696,
    "asset": {
      "asset_id": "ALVYPzjZjSLoT4xAHJ7x38RcHWNatP4Mba",
      "asset_quantity": 10
    }
  }
]
```

## blockchain.openassets.scripthash.listuncoloredunspent

Return an ordered list of uncolored UTXOs sent to a scripthash.

### Signature
  blockchain.openassets.scripthash.listuncoloredunspent(scripthash)

  scripthash: The script hash as a hexadecimal string.

### Result
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

### Result Example

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
