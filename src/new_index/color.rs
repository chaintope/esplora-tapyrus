use std::collections::HashMap;
use tapyrus::consensus::encode::{deserialize, serialize};
use tapyrus::ColorIdentifier;
use tapyrus::{BlockHash, OutPoint, Transaction, TxOut, Txid};

use crate::new_index::db::DBRow;
use crate::new_index::schema::FullHash;
use crate::util::{full_hash, Bytes};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct ColoredTxHistoryKey {
    pub color_id: ColorIdentifier,
    pub confirmed_height: u32,
    pub txinfo: ColoredTxHistoryInfo,
}

#[derive(Debug, PartialEq)]
pub struct ColoredTxHistoryRow {
    pub key: ColoredTxHistoryKey,
}

impl ColoredTxHistoryRow {
    fn new(
        color_id: &ColorIdentifier,
        confirmed_height: u32,
        txinfo: ColoredTxHistoryInfo,
    ) -> Self {
        let key = ColoredTxHistoryKey {
            color_id: color_id.clone(),
            confirmed_height,
            txinfo,
        };
        ColoredTxHistoryRow { key }
    }

    pub fn filter(color_id: &ColorIdentifier) -> Bytes {
        bincode::serialize(&(b'C', &serialize_color_id(color_id))).unwrap()
    }

    pub fn prefix_height(color_id: &ColorIdentifier, height: u32) -> Bytes {
        bincode::serialize(&(b'C', &serialize_color_id(color_id), height)).unwrap()
    }

    pub fn prefix_end(color_id: &ColorIdentifier) -> Bytes {
        bincode::serialize(&(b'C', &serialize_color_id(color_id), std::u32::MAX)).unwrap()
    }

    pub fn get_txid(&self) -> Txid {
        self.key.txinfo.get_txid()
    }

    fn into_row(self) -> DBRow {
        DBRow {
            key: bincode::serialize(&(
                b'C',
                &serialize_color_id(&self.key.color_id),
                self.key.confirmed_height,
                self.key.txinfo,
            ))
            .unwrap(),
            value: vec![],
        }
    }

    pub fn from_row(row: DBRow) -> Self {
        let (_prefix, token_type, payload, confirmed_height, txinfo): (
            u8,
            u8,
            [u8; 32],
            u32,
            ColoredTxHistoryInfo,
        ) = bincode::deserialize(&row.key).unwrap();
        ColoredTxHistoryRow {
            key: ColoredTxHistoryKey {
                color_id: deserialize_color_id(token_type, payload),
                confirmed_height,
                txinfo,
            },
        }
    }
}

pub fn serialize_color_id(color_id: &ColorIdentifier) -> (u8, [u8; 32]) {
    let color_id = serialize(color_id);
    let token_type: u8 = color_id[0];
    let mut payload: [u8; 32] = [0; 32];
    payload.copy_from_slice(&color_id[1..33]);
    (token_type, payload)
}

pub fn deserialize_color_id(token_type: u8, payload: [u8; 32]) -> ColorIdentifier {
    let mut v = vec![token_type];
    v.extend(payload.to_vec());
    ColorIdentifier::from_slice(&v).expect("failed to deserialize color_id")
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ColoredTxHistoryInfo {
    Issuing(IssuingInfo),
    Transferring(TransferringInfo),
    Burning(BurningInfo),
}

impl ColoredTxHistoryInfo {
    pub fn get_txid(&self) -> Txid {
        match self {
            ColoredTxHistoryInfo::Issuing(IssuingInfo { txid, .. })
            | ColoredTxHistoryInfo::Transferring(TransferringInfo { txid, .. })
            | ColoredTxHistoryInfo::Burning(BurningInfo { txid, .. }) => deserialize(txid),
        }
        .expect("cannot parse Txid")
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct IssuingInfo {
    pub txid: FullHash,
    pub value: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TransferringInfo {
    pub txid: FullHash,
    pub value: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct BurningInfo {
    pub txid: FullHash,
    pub value: u64,
}

#[derive(Serialize, Deserialize)]
pub struct ColoredStatsCacheKey {
    pub color_id: ColorIdentifier,
}

pub struct ColoredStatsCacheRow {
    pub key: ColoredStatsCacheKey,
    pub value: Bytes,
}

impl ColoredStatsCacheRow {
    pub fn new(color_id: &ColorIdentifier, stats: &ColoredStats, blockhash: &BlockHash) -> Self {
        ColoredStatsCacheRow {
            key: ColoredStatsCacheKey {
                color_id: color_id.clone(),
            },
            value: bincode::serialize(&(stats, blockhash)).unwrap(),
        }
    }

    pub fn key(color_id: &ColorIdentifier) -> Bytes {
        bincode::serialize(&(b'z', &serialize_color_id(&color_id))).unwrap()
    }

    pub fn into_row(self) -> DBRow {
        DBRow {
            key: bincode::serialize(&(b'z', &serialize_color_id(&self.key.color_id))).unwrap(),
            value: self.value,
        }
    }

    pub fn from_row(row: DBRow) -> Self {
        let (_prefix, token_type, payload): (u8, u8, [u8; 32]) =
            bincode::deserialize(&row.key).expect("failed to deserialize ColoredStatsCacheKey");
        ColoredStatsCacheRow {
            key: ColoredStatsCacheKey {
                color_id: deserialize_color_id(token_type, payload),
            },
            value: row.value,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ColoredStats {
    pub color_id: ColorIdentifier,
    pub tx_count: usize,
    pub issued_tx_count: usize,
    pub transferred_tx_count: usize,
    pub burned_tx_count: usize,
    pub issued_sum: u64,
    pub transferred_sum: u64,
    pub burned_sum: u64,
}

impl ColoredStats {
    pub fn new(color_id: &ColorIdentifier) -> Self {
        ColoredStats {
            color_id: color_id.clone(),
            tx_count: 0,
            issued_tx_count: 0,
            transferred_tx_count: 0,
            burned_tx_count: 0,
            issued_sum: 0,
            transferred_sum: 0,
            burned_sum: 0,
        }
    }
}
// collect histories of issuing/transferring/burining colored coins in specified tx.
pub fn index_confirmed_colored_tx(
    tx: &Transaction,
    confirmed_height: u32,
    previous_txos_map: &HashMap<OutPoint, TxOut>,
    rows: &mut Vec<DBRow>,
) {
    let history = colored_tx_history(tx, previous_txos_map);

    rows.extend(
        history.into_iter().map(|(color_id, info)| {
            colored_history_row(&color_id, confirmed_height, info).into_row()
        }),
    );
}

fn colored_history_row(
    color_id: &ColorIdentifier,
    confirmed_height: u32,
    txinfo: ColoredTxHistoryInfo,
) -> ColoredTxHistoryRow {
    let key = ColoredTxHistoryKey {
        color_id: color_id.clone(),
        confirmed_height,
        txinfo,
    };
    ColoredTxHistoryRow { key }
}

// Return hash map which key is color id and value is amount of value in TxOuts
fn get_amounts(outs: &Vec<TxOut>) -> HashMap<ColorIdentifier, u64> {
    let mut amounts = HashMap::<ColorIdentifier, u64>::new();
    for txo in outs {
        if let Some((color_id, _)) = txo.script_pubkey.split_color() {
            let amount = match amounts.get(&color_id) {
                Some(amount) => *amount,
                None => 0,
            };
            amounts.insert(color_id.clone(), amount + txo.value);
        }
    }
    amounts
}

// Return histories of issuing/transferring/burining colored coin.
pub fn colored_tx_history(
    tx: &Transaction,
    previous_txos_map: &HashMap<OutPoint, TxOut>,
) -> Vec<(ColorIdentifier, ColoredTxHistoryInfo)> {
    let mut previous = Vec::new();
    for input in tx.input.iter() {
        match previous_txos_map.get(&input.previous_output) {
            Some(txout) => {
                previous.push(txout.clone());
            }
            None => {}
        }
    }
    let colored_prevouts = get_amounts(&previous);
    let colored_outs = get_amounts(&tx.output);

    let txid = full_hash(&tx.malfix_txid()[..]);

    // key: ColorIdentifier
    // value: tuple of amount in prev output and in current output.
    let mut colored_map: HashMap<ColorIdentifier, (u64, u64)> = HashMap::new();

    for (color_id, prev_amount) in colored_prevouts {
        colored_map.insert(color_id.clone(), (prev_amount, 0));
    }
    for (color_id, amount) in colored_outs {
        let new_amount = match colored_map.get(&color_id) {
            Some((prev_amount, _)) => (*prev_amount, amount),
            None => (0, amount),
        };
        colored_map.insert(color_id.clone(), new_amount);
    }

    let mut tx_history = vec![];
    for (color_id, (prev_amount, amount)) in colored_map {
        for history_info in create_history_info(txid, prev_amount, amount) {
            tx_history.push((color_id.clone(), history_info));
        }
    }
    tx_history
}

pub fn create_history_info(
    txid: FullHash,
    prev_amount: u64,
    amount: u64,
) -> Vec<ColoredTxHistoryInfo> {
    let mut histories = vec![];
    if amount > prev_amount {
        histories.push(ColoredTxHistoryInfo::Issuing(IssuingInfo {
            txid,
            value: amount - prev_amount,
        }));
        if prev_amount > 0 {
            histories.push(ColoredTxHistoryInfo::Transferring(TransferringInfo {
                txid,
                value: prev_amount,
            }));
        }
    } else if amount == prev_amount {
        histories.push(ColoredTxHistoryInfo::Transferring(TransferringInfo {
            txid,
            value: amount,
        }));
    } else {
        histories.push(ColoredTxHistoryInfo::Burning(BurningInfo {
            txid,
            value: prev_amount - amount,
        }));
        if amount > 0 {
            histories.push(ColoredTxHistoryInfo::Transferring(TransferringInfo {
                txid,
                value: amount,
            }));
        }
    }
    histories
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex::FromHex;
    use tapyrus::hash_types::Txid;
    use tapyrus::{ColorIdentifier, OutPoint, Script, Transaction, TxIn, TxOut};

    fn hex_script(hex: &str) -> Script {
        return Script::from(Vec::from_hex(hex).unwrap());
    }

    #[test]
    fn test_converting_row() {
        let color_id = ColorIdentifier::reissuable(hex_script(
            "76a91437d8a6977e2b61459c594c8da713a2aeac7516b188ac",
        ));
        let txid = full_hash(&Txid::default()[..]);
        let txinfo = ColoredTxHistoryInfo::Issuing(IssuingInfo {
            txid: txid,
            value: 100,
        });
        let row = ColoredTxHistoryRow::new(&color_id, 10, txinfo.clone());
        let dbrow = row.into_row();
        let decoded = ColoredTxHistoryRow::from_row(dbrow);
        assert_eq!(decoded.key.color_id, color_id);
        assert_eq!(decoded.key.confirmed_height, 10);
        assert_eq!(decoded.key.txinfo, txinfo);
    }

    #[test]
    fn test_get_amounts() {
        let color_id1 = ColorIdentifier::nft(OutPoint::default());
        let color_id2 = ColorIdentifier::reissuable(hex_script(
            "76a91437d8a6977e2b61459c594c8da713a2aeac7516b188ac",
        ));
        let color_id3 = ColorIdentifier::reissuable(hex_script(
            "a9143545e6e33b832c47050f24d3eeb93c9c03948bc787",
        ));
        let p2pkh = hex_script("76a91437d8a6977e2b61459c594c8da713a2aeac7516b188ac");
        let p2sh = hex_script("a9143545e6e33b832c47050f24d3eeb93c9c03948bc787");
        let cp2pkh1 = p2pkh.add_color(color_id1.clone()).unwrap();
        let cp2pkh2 = p2pkh.add_color(color_id2.clone()).unwrap();
        let cp2pkh3 = p2pkh.add_color(color_id3.clone()).unwrap();
        let cp2sh1 = p2sh.add_color(color_id2.clone()).unwrap();

        let mut outs = vec![];
        outs.push(TxOut {
            script_pubkey: p2pkh,
            value: 500_000_000,
        });
        outs.push(TxOut {
            script_pubkey: cp2pkh1,
            value: 1,
        });
        outs.push(TxOut {
            script_pubkey: cp2pkh2,
            value: 50,
        });
        outs.push(TxOut {
            script_pubkey: cp2sh1,
            value: 50,
        });
        outs.push(TxOut {
            script_pubkey: cp2pkh3.clone(),
            value: 100,
        });
        outs.push(TxOut {
            script_pubkey: cp2pkh3,
            value: 100,
        });
        let amounts = get_amounts(&outs);
        assert_eq!(*amounts.get(&color_id1).unwrap(), 1);
        assert_eq!(*amounts.get(&color_id2).unwrap(), 100);
        assert_eq!(*amounts.get(&color_id3).unwrap(), 200);
    }

    #[test]
    fn test_create_history_info() {
        let txid = full_hash(&Txid::default()[..]);
        let histories = create_history_info(txid, 100, 300);
        assert_eq!(histories.len(), 2);
        assert_eq!(
            histories[0],
            ColoredTxHistoryInfo::Issuing(IssuingInfo { txid, value: 200 })
        );
        assert_eq!(
            histories[1],
            ColoredTxHistoryInfo::Transferring(TransferringInfo { txid, value: 100 })
        );

        let histories = create_history_info(txid, 0, 300);
        assert_eq!(histories.len(), 1);
        assert_eq!(
            histories[0],
            ColoredTxHistoryInfo::Issuing(IssuingInfo { txid, value: 300 })
        );

        let histories = create_history_info(txid, 200, 200);
        assert_eq!(histories.len(), 1);
        assert_eq!(
            histories[0],
            ColoredTxHistoryInfo::Transferring(TransferringInfo { txid, value: 200 })
        );

        let histories = create_history_info(txid, 400, 300);
        assert_eq!(histories.len(), 2);
        assert_eq!(
            histories[0],
            ColoredTxHistoryInfo::Burning(BurningInfo { txid, value: 100 })
        );
        assert_eq!(
            histories[1],
            ColoredTxHistoryInfo::Transferring(TransferringInfo { txid, value: 300 })
        );

        let histories = create_history_info(txid, 400, 0);
        assert_eq!(histories.len(), 1);
        assert_eq!(
            histories[0],
            ColoredTxHistoryInfo::Burning(BurningInfo { txid, value: 400 })
        );
    }

    #[test]
    fn test_index_confirmed_colored_tx() {
        use tapyrus::hashes::hex::FromHex;

        let p2pkh1 = hex_script("76a914000000000000000000000000000000000000000088ac");
        let p2pkh2 = hex_script("76a914111111111111111111111111111111111111111188ac");
        let p2pkh3 = hex_script("76a914222222222222222222222222222222222222222288ac");
        let out_point = OutPoint::new(
            Txid::from_hex("0000000000000000000000000000000000000000000000000000000000000000")
                .unwrap(),
            1,
        );

        let color_id1 = ColorIdentifier::reissuable(p2pkh1.clone());
        let color_id2 = ColorIdentifier::non_reissuable(out_point);

        let cp2pkh1 = p2pkh1.add_color(color_id1.clone()).unwrap();
        let cp2pkh2 = p2pkh2.add_color(color_id2.clone()).unwrap();
        let cp2pkh3 = p2pkh3.add_color(color_id2.clone()).unwrap();

        let out_point1 = OutPoint::new(
            Txid::from_hex("0101010101010101010101010101010101010101010101010101010101010101")
                .unwrap(),
            1,
        );
        let out_point2 = OutPoint::new(
            Txid::from_hex("0202020202020202020202020202020202020202020202020202020202020202")
                .unwrap(),
            1,
        );

        let mut previous_txos_map = HashMap::new();
        previous_txos_map.insert(
            out_point1,
            TxOut {
                value: 200,
                script_pubkey: cp2pkh1.clone(),
            },
        );
        previous_txos_map.insert(
            out_point2,
            TxOut {
                value: 200,
                script_pubkey: cp2pkh2.clone(),
            },
        );

        // color_id1 (reissuable)    : 200 -> 100 (100 burned, 100 transferred)
        // color_id2 (non-reissuable): 200 -> 300 (100 issued, 200 transferred)
        let tx = Transaction {
            version: 1,
            lock_time: 0,
            input: vec![
                TxIn {
                    previous_output: out_point1,
                    script_sig: Script::default(),
                    sequence: 0xffffffff,
                    witness: vec![],
                },
                TxIn {
                    previous_output: out_point2,
                    script_sig: Script::default(),
                    sequence: 0xffffffff,
                    witness: vec![],
                },
            ],
            output: vec![
                TxOut {
                    value: 100,
                    script_pubkey: cp2pkh1.clone(),
                },
                TxOut {
                    value: 300,
                    script_pubkey: cp2pkh3.clone(),
                },
            ],
        };
        let mut rows = vec![];
        index_confirmed_colored_tx(&tx, 10, &previous_txos_map, &mut rows);

        assert_eq!(rows.len(), 4);

        rows.sort_by(|a, b| a.key.cmp(&b.key));
        let row0 = rows.get(0).unwrap();
        let hex = hex::encode::<Vec<u8>>(row0.key.iter().cloned().collect());
        // field               | size | value                                                                 |
        //---------------------|------|-----------------------------------------------------------------------|
        // prefix              |    1 | 'C'(0x43)                                                             |
        // color_id            |   33 | c12dceb0cedd7c372c838fea8d46ae863a3c47b2ad0fb950e90ac9d531583ad35e    |
        // height              |    4 | 10(0x0a000000)                                                        |
        // Issue/Transfer/Burn |    4 | 'Transfer'(0x010000000)                                               |
        // txid                |   32 | 59abe954f5636c86484e5e2817d29b915e7f9a9f0294e87c438fd060694a8b1c      |
        // value               |    8 | 100(0x64)                                                             |
        assert_eq!(hex, "43c12dceb0cedd7c372c838fea8d46ae863a3c47b2ad0fb950e90ac9d531583ad35e0a0000000100000059abe954f5636c86484e5e2817d29b915e7f9a9f0294e87c438fd060694a8b1c6400000000000000");

        let row1 = rows.get(1).unwrap();
        let hex = hex::encode::<Vec<u8>>(row1.key.iter().cloned().collect());
        // field               | size | value                                                                 |
        //---------------------|------|-----------------------------------------------------------------------|
        // prefix              |    1 | 'C'(0x43)                                                             |
        // color_id            |   33 | c12dceb0cedd7c372c838fea8d46ae863a3c47b2ad0fb950e90ac9d531583ad35e    |
        // height              |    4 | 10(0x0a000000)                                                        |
        // Issue/Transfer/Burn |    4 | 'Burn'(0x02000000)                                                    |
        // txid                |   32 | 59abe954f5636c86484e5e2817d29b915e7f9a9f0294e87c438fd060694a8b1c      |
        // value               |    8 | 100(0x64)                                                             |
        assert_eq!(hex, "43c12dceb0cedd7c372c838fea8d46ae863a3c47b2ad0fb950e90ac9d531583ad35e0a0000000200000059abe954f5636c86484e5e2817d29b915e7f9a9f0294e87c438fd060694a8b1c6400000000000000");

        let row2 = rows.get(2).unwrap();
        let hex = hex::encode::<Vec<u8>>(row2.key.iter().cloned().collect());
        // field               | size | value                                                                 |
        //---------------------|------|-----------------------------------------------------------------------|
        // prefix              |    1 | 'C'(0x43)                                                             |
        // color_id            |   33 | c271c99cc3bc21757feed5b712744ebb0f770d5c41d99189f9457495747bf11050    |
        // height              |    4 | 10(0x0a000000)                                                        |
        // Issue/Transfer/Burn |    4 | 'Issue'(0x00000000)                                                   |
        // txid                |   32 | 59abe954f5636c86484e5e2817d29b915e7f9a9f0294e87c438fd060694a8b1c      |
        // value               |    8 | 100(0x64)                                                             |
        assert_eq!(hex, "43c271c99cc3bc21757feed5b712744ebb0f770d5c41d99189f9457495747bf110500a0000000000000059abe954f5636c86484e5e2817d29b915e7f9a9f0294e87c438fd060694a8b1c6400000000000000");

        let row3 = rows.get(3).unwrap();
        let hex = hex::encode::<Vec<u8>>(row3.key.iter().cloned().collect());
        // field               | size | value                                                                 |
        //---------------------|------|-----------------------------------------------------------------------|
        // prefix              |    1 | 'C'(0x43)                                                             |
        // color_id            |   33 | c271c99cc3bc21757feed5b712744ebb0f770d5c41d99189f9457495747bf11050    |
        // height              |    4 | 10(0x0a000000)                                                        |
        // Issue/Transfer/Burn |    4 | 'Transfer'(0x01000000)                                                |
        // txid                |   32 | 59abe954f5636c86484e5e2817d29b915e7f9a9f0294e87c438fd060694a8b1c      |
        // value               |    8 | 200(0xc8)                                                             |
        assert_eq!(hex, "43c271c99cc3bc21757feed5b712744ebb0f770d5c41d99189f9457495747bf110500a0000000100000059abe954f5636c86484e5e2817d29b915e7f9a9f0294e87c438fd060694a8b1cc800000000000000");
    }
}
