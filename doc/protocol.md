# Electrum Protocol for Electrs-Tapyrus

This document describes available APIs in electrs-tapyrus

electrs-tapyrus is based on electrs which supports Electrum Protocol, so almost all APIs are available as is.

In this document, we detail about the APIs which have differences from original APIs.

For detail of original APIs, see [Electrum Protocol](https://electrumx.readthedocs.io/en/latest/protocol.html)

## Protocol Methods


### blockchain.scripthash.get_balance

Return the confirmed and unconfirmed balances of a `script hash`.

**Signature**

    blockchain.scripthash.get_balance(scripthash)

  *scripthash*

    The script hash as a hexadecimal string.

**Result**

  An array of dictionary with keys `confirmed`, `color_id`, and `unconfirmed`.  The value of each is the appropriate balance in coin units as a string.
  A list of balance in coin. Each balance is a dictionary with the following keys:

  * *confirmed*

    confirmed amount. The value is the appropriate balance in coin units as a string.

  * *unconfirmed*

    unconfirmed amount. The value is the appropriate balance in coin units as a string.

  * *color_id*

    The color identifier for colored coin.

**Result Example**

```
  [{
    "confirmed": "1.03873966",
    "unconfirmed": "0.236844"
  },{
    "color_id": "c3ec2fd806701a3f55808cbec3922c38dafaa3070c48c803e9043ee3642c660b46"
    "confirmed": "1",
    "unconfirmed": "0"
  },{
    "color_id": "c1c4d56b5da93862de658dbc26f8f0585668609ad3f3416543a6aa81d4dfb7cc08"
    "confirmed": "100",
    "unconfirmed": "1"
  }]
```

### blockchain.scripthash.listunspent


Return an ordered list of UTXOs sent to a script hash including native token(TPC) and colored token.
If the script hash of cp2pkh or cp2sh is specified, this returns list of colored token.
If the script hash of p2pkh or p2sh is specified, this returns list of not only native token(TPC) but also colored token derived from it.

**Signature**

    blockchain.scripthash.listunspent(scripthash)

  *scripthash*

    The script hash as a hexadecimal string.

**Result**

  A list of unspent outputs in blockchain order.  This function takes the mempool into account.  Mempool transactions paying to the address are included at the end of the list in an undefined order.
  Any output that is spent in the mempool does not appear. Each output is a dictionary with the following keys:

  * *height*

    The integer height of the block the transaction was confirmed in.
    ``0`` if the transaction is in the mempool.

  * *tx_pos*

    The zero-based index of the output in the transaction's list of
    outputs.

  * *tx_hash*

    The output's transaction hash as a hexadecimal string.

  * *value*

    The output's value in minimum coin units (tapyrus).

  * *color_id*

    The color identifier of the output for colored coin.

**Result Example**

```
  [
    {
      "tx_pos": 0,
      "value": 45318048,
      "tx_hash": "9f2c45a12db0144909b5db269415f7319179105982ac70ed80d76ea79d923ebf",
      "height": 437146,
      "color_id": "c3ec2fd806701a3f55808cbec3922c38dafaa3070c48c803e9043ee3642c660b46"
    },
    {
      "tx_pos": 0,
      "value": 919195,
      "tx_hash": "3d2290c93436a3e964cfc2f0950174d8847b1fbe3946432c4784e168da0f019f",
      "height": 441696,
    }
  ]
```

### blockchain.scripthash.listcoloredunspent

Return an ordered list of colored UTXOs sent to a script hash.
If the script hash of cp2pkh or cp2sh is specified, this returns list of colored token.
If the script hash of p2pkh or p2sh is specified, this returns list of colored token derived from it.

**Signature**

  blockchain.scripthash.listcoloredunspent(scripthash, color_id = '000000000000000000000000000000000000000000000000000000000000000000')

  *scripthash*

    The script hash as a hexadecimal string.

  *color_id*

    The color identifier as a hexadecimal string. if color_id is not specified, return all colored coin associated with this scripthash.

**Result**

  A list of colored unspent outputs in blockchain order. This function takes the mempool into account.
  Mempool transactions paying to the address are included at the end of the list in an undefined order.
  Any output that is spent in the mempool does not appear.
  Each output is a dictionary with the following keys:

  * *height*

    The integer height of the block the transaction was confirmed in.
    ``0`` if the transaction is in the mempool.

  * *tx_pos*

    The zero-based index of the output in the transaction's list of
    outputs.

  * *tx_hash*

    The output's transaction hash as a hexadecimal string.

  * *value*

    The output's value in minimum coin units (tapyrus).

  * *color_id*

    The color identifier of the output for colored coin.

**Result Example**

```
  [
    {
      "tx_pos": 0,
      "value": 45318048,
      "tx_hash": "9f2c45a12db0144909b5db269415f7319179105982ac70ed80d76ea79d923ebf",
      "height": 437146,
      "color_id": "c3ec2fd806701a3f55808cbec3922c38dafaa3070c48c803e9043ee3642c660b46"
    },
    {
      "tx_pos": 0,
      "value": 919195,
      "tx_hash": "3d2290c93436a3e964cfc2f0950174d8847b1fbe3946432c4784e168da0f019f",
      "height": 441696,
      "color_id": "c3ec2fd806701a3f55808cbec3922c38dafaa3070c48c803e9043ee3642c660b46"
    }
  ]
```

### blockchain.scripthash.listuncoloredunspent

Return an ordered list of uncolored (i.e. Native token) UTXOs sent to a script hash.

**Signature**

  blockchain.scripthash.listuncoloredunspent(scripthash)

  *scripthash*

    The script hash as a hexadecimal string.

**Result**

  A list of uncolored unspent outputs in blockchain order.  This function takes the mempool into account.
  Mempool transactions paying to the address are included at the end of the list in an undefined order.
  Any output that is spent in the mempool does not appear.  Each output is a dictionary with the following keys:

  * *height*

    The integer height of the block the transaction was confirmed in.
    ``0`` if the transaction is in the mempool.

  * *tx_pos*

    The zero-based index of the output in the transaction's list of
    outputs.

  * *tx_hash*

    The output's transaction hash as a hexadecimal string.

  * *value*

    The output's value in minimum coin units (tapyrus).

**Result Example**

```
  [
    {
      "tx_pos": 0,
      "value": 45318048,
      "tx_hash": "9f2c45a12db0144909b5db269415f7319179105982ac70ed80d76ea79d923ebf",
      "height": 437146,
    },
    {
      "tx_pos": 0,
      "value": 919195,
      "tx_hash": "3d2290c93436a3e964cfc2f0950174d8847b1fbe3946432c4784e168da0f019f",
      "height": 441696,
    }
  ]
```
