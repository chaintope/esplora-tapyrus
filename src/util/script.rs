use tapyrus::blockdata::script::{Instruction::PushBytes, Script};

use crate::chain::Network;
use crate::chain::{TxIn, TxOut};

pub struct InnerScripts {
    pub redeem_script: Option<Script>,
    pub witness_script: Option<Script>,
}

pub fn script_to_address(script: &Script, network: Network) -> Option<String> {
    match network {
        _ => tapyrus::Address::from_script(script, network.into()).map(|s| s.to_string()),
    }
}

pub fn get_script_asm(script: &Script) -> String {
    let asm = format!("{:?}", script);
    (&asm[7..asm.len() - 1]).to_string()
}

pub fn get_script_type(script: &Script) -> String {
    let script_type = if script.is_empty() {
        "empty"
    } else if script.is_op_return() {
        "op_return"
    } else if script.is_p2pk() {
        "p2pk"
    } else if script.is_p2pkh() {
        "p2pkh"
    } else if script.is_p2sh() {
        "p2sh"
    } else if script.is_provably_unspendable() {
        "provably_unspendable"
    } else if script.is_cp2pkh() {
        "cp2pkh"
    } else if script.is_cp2sh() {
        "cp2sh"
    } else {
        "unknown"
    };
    return script_type.to_string();
}

// Returns the witnessScript in the case of p2wsh, or the redeemScript in the case of p2sh.
pub fn get_innerscripts(txin: &TxIn, prevout: &TxOut) -> InnerScripts {
    // Wrapped redeemScript for P2SH spends
    let redeem_script = if prevout.script_pubkey.is_p2sh() {
        if let Some(Ok(PushBytes(redeemscript))) = txin.script_sig.instructions_minimal().last() {
            Some(Script::from(redeemscript.to_vec()))
        } else {
            None
        }
    } else {
        None
    };

    // Wrapped witnessScript for P2WSH or P2SH-P2WSH spends
    let witness_script = if prevout.script_pubkey.is_v0_p2wsh()
        || redeem_script.as_ref().map_or(false, |s| s.is_v0_p2wsh())
    {
        let witness = &txin.witness;
        witness.iter().last().cloned().map(Script::from)
    } else {
        None
    };

    InnerScripts {
        redeem_script,
        witness_script,
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::chain::Network;
    use hex::FromHex;
    use tapyrus::Script;

    fn hex_script(hex: &str) -> Script {
        return Script::from(Vec::from_hex(hex).unwrap());
    }

    #[test]
    fn test_script_to_address() {
        let script = hex_script("76a91437d8a6977e2b61459c594c8da713a2aeac7516b188ac");
        let address = script_to_address(&script, Network::new("prod", 1)).unwrap();
        assert_eq!(address, "166Hhi6oFtpMmugcLY2uyBFDC6caDFjT9s".to_string());

        let script = hex_script("21c13c630f9d53c11847a662c963dfb1e05a8630dcb901262533cb2f590c480cc734bc76a91437d8a6977e2b61459c594c8da713a2aeac7516b188ac");
        let address = script_to_address(&script, Network::new("prod", 1)).unwrap();
        assert_eq!(
            address,
            "vhUcGQDo5abu4Ld1azfVWyzTDxFzYu9kk93VJ9ozv8zjq8vaGtGemPHKXfRnxPzbNXaxSk5sxd8Neh"
                .to_string()
        );
    }

    #[test]
    fn test_get_script_type() {
        let p2pkh = hex_script("76a91437d8a6977e2b61459c594c8da713a2aeac7516b188ac");
        assert_eq!(get_script_type(&p2pkh), "p2pkh".to_string());

        let p2sh = hex_script("a9143545e6e33b832c47050f24d3eeb93c9c03948bc787");
        assert_eq!(get_script_type(&p2sh), "p2sh".to_string());

        let cp2pkh = hex_script("21c13c630f9d53c11847a662c963dfb1e05a8630dcb901262533cb2f590c480cc734bc76a91437d8a6977e2b61459c594c8da713a2aeac7516b188ac");
        assert_eq!(get_script_type(&cp2pkh), "cp2pkh".to_string());

        let cp2sh = hex_script("21c13c630f9d53c11847a662c963dfb1e05a8630dcb901262533cb2f590c480cc734bca9143545e6e33b832c47050f24d3eeb93c9c03948bc787");
        assert_eq!(get_script_type(&cp2sh), "cp2sh".to_string());
    }
}
