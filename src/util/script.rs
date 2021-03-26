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
    } else if script.is_v0_p2wpkh() {
        "v0_p2wpkh"
    } else if script.is_v0_p2wsh() {
        "v0_p2wsh"
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
