use std::collections::HashMap;

/// The type-checking sequence in which we must compile the modules.
/// In a 'real' project, this is done using a dependency graph which
/// code lies under aiken-project -- not importable here.
pub const MODULES_SEQUENCE: [&str; 27] = [
    "aiken/crypto",
    "aiken/math",
    "aiken/option",
    "aiken/primitive/bytearray",
    "aiken/primitive/int",
    "aiken/collection",
    "aiken/collection/dict",
    "aiken/collection/list",
    "aiken/math/rational",
    "aiken/cbor",
    "aiken/collection/pairs",
    "aiken/interval",
    "aiken/crypto/bls12_381/scalar",
    "aiken/crypto/bls12_381/g1",
    "aiken/crypto/bls12_381/g2",
    "cardano/address",
    "cardano/address/credential",
    "cardano/assets",
    "cardano/governance/protocol_parameters",
    "cardano/certificate",
    "aiken/primitive/string",
    "cardano/governance",
    "cardano/governance/voter",
    "cardano/transaction",
    "cardano/transaction/output_reference",
    "cardano/script_context",
    "cardano/transaction/script_purpose",
];

pub fn modules() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("aiken/cbor", include_str!("../../stdlib/lib/aiken/cbor.ak"));
    m.insert("aiken/collection", include_str!("../../stdlib/lib/aiken/collection.ak"));
    m.insert("aiken/collection/dict", include_str!("../../stdlib/lib/aiken/collection/dict.ak"));
    m.insert("aiken/collection/list", include_str!("../../stdlib/lib/aiken/collection/list.ak"));
    m.insert("aiken/collection/pairs", include_str!("../../stdlib/lib/aiken/collection/pairs.ak"));
    m.insert("aiken/crypto", include_str!("../../stdlib/lib/aiken/crypto.ak"));
    m.insert("aiken/crypto/bls12_381/g1", include_str!("../../stdlib/lib/aiken/crypto/bls12_381/g1.ak"));
    m.insert("aiken/crypto/bls12_381/g2", include_str!("../../stdlib/lib/aiken/crypto/bls12_381/g2.ak"));
    m.insert("aiken/crypto/bls12_381/scalar", include_str!("../../stdlib/lib/aiken/crypto/bls12_381/scalar.ak"));
    m.insert("aiken/interval", include_str!("../../stdlib/lib/aiken/interval.ak"));
    m.insert("aiken/math", include_str!("../../stdlib/lib/aiken/math.ak"));
    m.insert("aiken/math/rational", include_str!("../../stdlib/lib/aiken/math/rational.ak"));
    m.insert("aiken/option", include_str!("../../stdlib/lib/aiken/option.ak"));
    m.insert("aiken/primitive/bytearray", include_str!("../../stdlib/lib/aiken/primitive/bytearray.ak"));
    m.insert("aiken/primitive/int", include_str!("../../stdlib/lib/aiken/primitive/int.ak"));
    m.insert("aiken/primitive/string", include_str!("../../stdlib/lib/aiken/primitive/string.ak"));
    m.insert("cardano/address", include_str!("../../stdlib/lib/cardano/address.ak"));
    m.insert("cardano/address/credential", include_str!("../../stdlib/lib/cardano/address/credential.ak"));
    m.insert("cardano/assets", include_str!("../../stdlib/lib/cardano/assets.ak"));
    m.insert("cardano/certificate", include_str!("../../stdlib/lib/cardano/certificate.ak"));
    m.insert("cardano/governance", include_str!("../../stdlib/lib/cardano/governance.ak"));
    m.insert("cardano/governance/protocol_parameters", include_str!("../../stdlib/lib/cardano/governance/protocol_parameters.ak"));
    m.insert("cardano/governance/voter", include_str!("../../stdlib/lib/cardano/governance/voter.ak"));
    m.insert("cardano/script_context", include_str!("../../stdlib/lib/cardano/script_context.ak"));
    m.insert("cardano/transaction", include_str!("../../stdlib/lib/cardano/transaction.ak"));
    m.insert("cardano/transaction/output_reference", include_str!("../../stdlib/lib/cardano/transaction/output_reference.ak"));
    m.insert("cardano/transaction/script_purpose", include_str!("../../stdlib/lib/cardano/transaction/script_purpose.ak"));
    m
}
