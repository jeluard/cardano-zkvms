#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use openvm_mcu_verifier_core::{
    decode_message, encode_frame, halo2_backend, FrameType, OpenVmEvmHalo2Verifier, ProofEnvelope,
    ProofKind, ProtocolError, VerificationReport, Verifier, VerifierKey, VerifyError,
    OPENVM_EVM_HALO2_PROOF_DATA_LEN,
};
use serde::{Deserialize, Serialize};

pub mod transfer;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProofStatus {
    Verified,
    Rejected,
    CryptoBackendUnavailable,
}

impl ProofStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Rejected => "rejected",
            Self::CryptoBackendUnavailable => "crypto backend unavailable",
        }
    }

    pub const fn detail(self) -> &'static str {
        match self {
            Self::Verified => "OpenVM proof accepted",
            Self::Rejected => "proof did not verify",
            Self::CryptoBackendUnavailable => "no no_std Halo2/KZG backend linked",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProofProbe {
    pub status: ProofStatus,
    pub host: HostProofSummary,
    pub proof_kind: &'static str,
    pub public_values_len: usize,
    pub proof_data_len: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostProofSummary {
    pub embedded: bool,
    pub status: Cow<'static, str>,
    pub detail: Cow<'static, str>,
    pub proof_sha: Cow<'static, str>,
    pub public_values: Cow<'static, str>,
}

pub fn proof_status_from_result(result: Result<VerificationReport, VerifyError>) -> ProofStatus {
    match result {
        Ok(report) if report.verified => ProofStatus::Verified,
        Ok(_) => ProofStatus::Rejected,
        Err(VerifyError::CryptoBackendUnavailable) => ProofStatus::CryptoBackendUnavailable,
        Err(_) => ProofStatus::Rejected,
    }
}

pub fn verify_received_artifacts(
    verifier_key_bytes: &[u8],
    proof_envelope_bytes: &[u8],
    proof_sha: String,
    on_step: fn(u8, &'static str),
) -> Result<ProofProbe, ()> {
    let key = decode_message::<VerifierKey>(verifier_key_bytes).map_err(|_| ())?;
    let proof = decode_message::<ProofEnvelope>(proof_envelope_bytes).map_err(|_| ())?;
    let public_values = short_hex(&proof.user_public_values);
    let public_values_len = proof.user_public_values.len();
    let proof_data_len = proof.proof_data.len();
    let mut verifier = OpenVmEvmHalo2Verifier { on_step };
    let verify_result = verifier.verify(&key, &proof);
    let (status, detail): (ProofStatus, Cow<'static, str>) = match &verify_result {
        Ok(report) if report.verified => (ProofStatus::Verified, Cow::Borrowed(ProofStatus::Verified.detail())),
        Ok(_) => (ProofStatus::Rejected, Cow::Borrowed(ProofStatus::Rejected.detail())),
        Err(VerifyError::CryptoBackendUnavailable) => (ProofStatus::CryptoBackendUnavailable, Cow::Borrowed(ProofStatus::CryptoBackendUnavailable.detail())),
        Err(e) => (ProofStatus::Rejected, Cow::Owned(alloc::format!("{}", e))),
    };

    Ok(ProofProbe {
        status,
        host: HostProofSummary {
            embedded: false,
            status: Cow::Borrowed("BLE received"),
            detail,
            proof_sha: Cow::Owned(proof_sha),
            public_values: Cow::Owned(public_values),
        },
        proof_kind: "OpenVM Halo2/KZG",
        public_values_len,
        proof_data_len,
    })
}

pub fn no_std_halo2_probe() -> ProofProbe {
    let key = VerifierKey::new(ProofKind::OpenVmEvmHalo2, [8; 32], vec![1]);
    let proof = ProofEnvelope::new_evm_halo2(
        "v2.0".into(),
        key.key_id,
        [1; 32],
        [2; 32],
        vec![42],
        vec![7; OPENVM_EVM_HALO2_PROOF_DATA_LEN],
    );
    let mut verifier = OpenVmEvmHalo2Verifier::default();
    let status = proof_status_from_result(verifier.verify(&key, &proof));
    let backend = halo2_backend();

    ProofProbe {
        status,
        host: HostProofSummary {
            embedded: false,
            status: Cow::Borrowed(backend.label()),
            detail: Cow::Borrowed(backend.detail()),
            proof_sha: Cow::Borrowed("no-std-probe"),
            public_values: Cow::Borrowed("2a"),
        },
        proof_kind: "OpenVM Halo2/KZG",
        public_values_len: proof.user_public_values.len(),
        proof_data_len: proof.proof_data.len(),
    }
}

fn short_hex(bytes: &[u8]) -> String {
    let mut text = String::new();
    for byte in bytes.iter().take(8) {
        use core::fmt::Write;
        let _ = write!(&mut text, "{byte:02x}");
    }
    text
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VerificationResponse {
    pub accepted: bool,
    pub verified: bool,
    pub report: Option<VerificationReport>,
    pub error: Option<String>,
}

impl VerificationResponse {
    fn success(report: VerificationReport) -> Self {
        Self {
            accepted: true,
            verified: report.verified,
            report: Some(report),
            error: None,
        }
    }

    fn failure(error: String) -> Self {
        Self {
            accepted: false,
            verified: false,
            report: None,
            error: Some(error),
        }
    }
}

pub struct Device<V> {
    verifier: V,
    verifier_key: VerifierKey,
}

impl<V: Verifier> Device<V> {
    pub fn new(verifier: V, verifier_key: VerifierKey) -> Self {
        Self {
            verifier,
            verifier_key,
        }
    }

    pub fn handle_proof_payload(&mut self, payload: &[u8]) -> Result<Vec<u8>, ProtocolError> {
        let response = match decode_message::<ProofEnvelope>(payload) {
            Ok(proof) => match self.verifier.verify(&self.verifier_key, &proof) {
                Ok(report) => VerificationResponse::success(report),
                Err(error) => VerificationResponse::failure(error.to_string()),
            },
            Err(error) => VerificationResponse::failure(error.to_string()),
        };

        encode_frame(FrameType::VerificationResponse, &response)
    }
}

pub fn verify_error_to_response(error: VerifyError) -> VerificationResponse {
    VerificationResponse::failure(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use openvm_mcu_verifier_core::{encode_message, ProofKind, OPENVM_EVM_HALO2_PROOF_DATA_LEN};
    use openvm_mcu_verifier_core::StructuralOnlyVerifier;

    #[test]
    fn device_processes_structural_fixture() {
        let key = VerifierKey::new(ProofKind::OpenVmEvmHalo2, [8; 32], vec![1]);
        let proof = ProofEnvelope::new_evm_halo2(
            "v2.0".into(),
            key.key_id,
            [1; 32],
            [2; 32],
            vec![42],
            vec![7; OPENVM_EVM_HALO2_PROOF_DATA_LEN],
        );
        let payload = encode_message(&proof).unwrap();
        let mut device = Device::new(StructuralOnlyVerifier, key);
        let response_frame = device.handle_proof_payload(&payload).unwrap();
        assert!(!response_frame.is_empty());
    }
}
