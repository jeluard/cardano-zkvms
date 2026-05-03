use alloc::borrow::Cow;

use openvm_mcu_device_app::{
    proof_status_from_result, verify_received_artifacts as verify_received_artifacts_common,
    HostProofSummary,
};
use openvm_mcu_verifier_core::{
    decode_message, ProofEnvelope, ProofKind, VerifierKey, OPENVM_EVM_HALO2_PROOF_DATA_LEN,
};
use openvm_mcu_verifier_core::{OpenVmEvmHalo2Verifier, Verifier};

#[allow(unused_imports)]
pub use openvm_mcu_device_app::{ProofProbe, ProofStatus};

#[allow(dead_code)]
mod host_status {
    include!(concat!(env!("OUT_DIR"), "/host_status.rs"));
}

pub fn run_probe() -> ProofProbe {
    let (key, proof) = embedded_proof_and_key().unwrap_or_else(dummy_proof_and_key);

    let mut verifier = OpenVmEvmHalo2Verifier::default();
    let status = proof_status_from_result(verifier.verify(&key, &proof));

    ProofProbe {
        status,
        host: HostProofSummary {
            embedded: host_status::HOST_EVM_EMBEDDED,
            status: Cow::Borrowed(host_status::HOST_EVM_STATUS),
            detail: Cow::Borrowed(host_status::HOST_EVM_DETAIL),
            proof_sha: Cow::Borrowed(host_status::HOST_EVM_PROOF_SHA),
            public_values: Cow::Borrowed(host_status::HOST_EVM_PUBLIC_VALUES),
        },
        proof_kind: "OpenVM Halo2/KZG",
        public_values_len: proof.user_public_values.len(),
        proof_data_len: proof.proof_data.len(),
    }
}

#[allow(dead_code)]
pub fn verify_received_artifacts(
    verifier_key_bytes: &[u8],
    proof_envelope_bytes: &[u8],
    proof_sha: alloc::string::String,
    on_step: fn(u8, &'static str),
) -> Result<ProofProbe, ()> {
    verify_received_artifacts_common(verifier_key_bytes, proof_envelope_bytes, proof_sha, on_step)
}

fn embedded_proof_and_key() -> Option<(VerifierKey, ProofEnvelope)> {
    if host_status::HOST_VERIFIER_KEY.is_empty() || host_status::HOST_PROOF_ENVELOPE.is_empty() {
        return None;
    }
    let key = decode_message(host_status::HOST_VERIFIER_KEY).ok()?;
    let proof = decode_message(host_status::HOST_PROOF_ENVELOPE).ok()?;
    Some((key, proof))
}

fn dummy_proof_and_key() -> (VerifierKey, ProofEnvelope) {
    let key = VerifierKey::new(ProofKind::OpenVmEvmHalo2, [8; 32], alloc::vec![1]);
    let proof = ProofEnvelope::new_evm_halo2(
        "v2.0".into(),
        key.key_id,
        [1; 32],
        [2; 32],
        alloc::vec![42],
        alloc::vec![7; OPENVM_EVM_HALO2_PROOF_DATA_LEN],
    );
    (key, proof)
}
