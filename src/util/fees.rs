use crate::chain::{Network, Transaction, TxOut};
use std::collections::HashMap;

const VSIZE_BIN_WIDTH: u32 = 50_000; // in vbytes

pub struct TxFeeInfo {
    pub fee: u64,   // in satoshis
    pub vsize: u32, // in virtual bytes (= weight/4)
    pub fee_per_vbyte: f32,
}

impl TxFeeInfo {
    pub fn new(tx: &Transaction, prevouts: &HashMap<u32, &TxOut>, network: Network) -> Self {
        let fee = get_tx_fee(tx, prevouts, network);
        let vsize = tx.get_weight() / 4;

        TxFeeInfo {
            fee,
            vsize: vsize as u32,
            fee_per_vbyte: fee as f32 / vsize as f32,
        }
    }
}

pub fn get_tx_fee(tx: &Transaction, prevouts: &HashMap<u32, &TxOut>, _network: Network) -> u64 {
    if tx.is_coin_base() {
        return 0;
    }

    let total_in: u64 = prevouts
        .values()
        .filter(|prevout| !prevout.script_pubkey.is_colored())
        .map(|prevout| prevout.value)
        .sum();
    let total_out: u64 = tx
        .output
        .iter()
        .filter(|vout| !vout.script_pubkey.is_colored())
        .map(|vout| vout.value)
        .sum();
    total_in - total_out
}

pub fn make_fee_histogram(mut entries: Vec<&TxFeeInfo>) -> Vec<(f32, u32)> {
    entries.sort_unstable_by(|e1, e2| e1.fee_per_vbyte.partial_cmp(&e2.fee_per_vbyte).unwrap());

    let mut histogram = vec![];
    let mut bin_size = 0;
    let mut last_fee_rate = 0.0;
    for e in entries.iter().rev() {
        if bin_size > VSIZE_BIN_WIDTH && last_fee_rate != e.fee_per_vbyte {
            // vsize of transactions paying >= last_fee_rate
            histogram.push((last_fee_rate, bin_size));
            bin_size = 0;
        }
        last_fee_rate = e.fee_per_vbyte;
        bin_size += e.vsize;
    }
    if bin_size > 0 {
        histogram.push((last_fee_rate, bin_size));
    }
    histogram
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex::FromHex;
    use std::collections::HashMap;
    use tapyrus::{Script, Transaction, TxOut};

    #[test]
    fn test_get_tx_fee() {
        let mut tx = Transaction {
            version: 1,
            lock_time: 0,
            input: vec![],
            output: vec![],
        };
        let uncolored =
            Vec::from_hex("76a91437d8a6977e2b61459c594c8da713a2aeac7516b188ac").unwrap();
        let colored = Vec::from_hex("21c13c630f9d53c11847a662c963dfb1e05a8630dcb901262533cb2f590c480cc734bc76a91437d8a6977e2b61459c594c8da713a2aeac7516b188ac").unwrap();

        tx.output.push(TxOut {
            value: 9500,
            script_pubkey: Script::from(uncolored.clone()),
        });
        tx.output.push(TxOut {
            value: 19000,
            script_pubkey: Script::from(colored.clone()),
        });

        let mut prevouts: HashMap<u32, &TxOut> = HashMap::new();
        let out1 = TxOut {
            value: 10000,
            script_pubkey: Script::from(uncolored.clone()),
        };
        let out2 = TxOut {
            value: 20000,
            script_pubkey: Script::from(colored.clone()),
        };
        prevouts.insert(0, &out1);
        prevouts.insert(1, &out2);
        let network: Network = Network::new("prod", 1);

        let fee = get_tx_fee(&tx, &prevouts, network);
        // 10000 - 9500 = 500
        assert_eq!(fee, 500);
    }
}
