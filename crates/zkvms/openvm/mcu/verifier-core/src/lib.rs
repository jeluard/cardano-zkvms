#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use core::fmt;

pub mod protocol;
pub use protocol::*;

mod halo2_compact;
mod halo2_portable;

pub use halo2_compact::*;
pub use halo2_portable::{debug_verify_frontend, PortableHalo2FrontendReport};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Halo2Backend {
    NativeStd,
    PortableNoStd,
    Unavailable,
}

impl Halo2Backend {
    pub const fn label(self) -> &'static str {
        match self {
            Self::NativeStd => "native std Halo2/KZG",
            Self::PortableNoStd => "portable no_std Halo2/KZG",
            Self::Unavailable => "no no_std Halo2/KZG backend",
        }
    }

    pub const fn detail(self) -> &'static str {
        match self {
            Self::NativeStd => "snark-verifier-sdk backend linked",
            Self::PortableNoStd => "local PLONK/SHPLONK/KZG backend linked",
            Self::Unavailable => "portable Halo2/KZG backend is not linked for this target",
        }
    }
}

pub const fn halo2_backend() -> Halo2Backend {
    if cfg!(feature = "halo2-std") {
        Halo2Backend::NativeStd
    } else {
        Halo2Backend::PortableNoStd
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerifyError {
    UnsupportedProtocolVersion { expected: u16, actual: u16 },
    ProofKindMismatch,
    VerifierKeyMismatch,
    EmptyPublicValues,
    EmptyVerifierKey,
    InvalidEvmHalo2ProofDataLen { expected: usize, actual: usize },
    InvalidHalo2VerifierKey,
    InvalidHalo2FieldElement,
    Halo2VerificationFailed,
    EmptyCommitment,
    MissingStarkBaseline,
    InvalidUtf8,
    StarkVerificationFailed,
    CryptoBackendUnavailable,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedProtocolVersion { expected, actual } => {
                write!(
                    formatter,
                    "unsupported protocol version {actual}, expected {expected}"
                )
            }
            Self::ProofKindMismatch => {
                formatter.write_str("proof kind does not match verifier key")
            }
            Self::VerifierKeyMismatch => {
                formatter.write_str("proof references a different verifier key")
            }
            Self::EmptyPublicValues => formatter.write_str("proof has no public values"),
            Self::EmptyVerifierKey => formatter.write_str("verifier key payload is empty"),
            Self::InvalidEvmHalo2ProofDataLen { expected, actual } => {
                write!(
                    formatter,
                    "invalid OpenVM Halo2/KZG proof data length {actual}, expected {expected}"
                )
            }
            Self::InvalidHalo2VerifierKey => {
                formatter.write_str("invalid Halo2 verifier key payload")
            }
            Self::InvalidHalo2FieldElement => {
                formatter.write_str("OpenVM Halo2/KZG proof contains an invalid field element")
            }
            Self::Halo2VerificationFailed => {
                formatter.write_str("OpenVM Halo2/KZG verification failed")
            }
            Self::EmptyCommitment => formatter.write_str("proof has an empty app commitment"),
            Self::MissingStarkBaseline => formatter
                .write_str("STARK proof envelope is missing verification baseline metadata"),
            Self::InvalidUtf8 => {
                formatter.write_str("STARK proof envelope contains non-UTF-8 JSON bytes")
            }
            Self::StarkVerificationFailed => {
                formatter.write_str("OpenVM STARK proof verification failed")
            }
            Self::CryptoBackendUnavailable => {
                formatter.write_str("no complete no_std Halo2/KZG backend is linked")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for VerifyError {}

pub trait Verifier {
    fn verify(
        &mut self,
        key: &VerifierKey,
        proof: &ProofEnvelope,
    ) -> Result<VerificationReport, VerifyError>;
}

fn noop_step(_: u8, _: &'static str) {}

pub struct OpenVmEvmHalo2Verifier {
    pub on_step: fn(u8, &'static str),
}

impl Default for OpenVmEvmHalo2Verifier {
    fn default() -> Self {
        Self { on_step: noop_step }
    }
}

impl Verifier for OpenVmEvmHalo2Verifier {
    fn verify(
        &mut self,
        key: &VerifierKey,
        proof: &ProofEnvelope,
    ) -> Result<VerificationReport, VerifyError> {
        validate_evm_halo2_inputs(key, proof)?;

        // When the native std backend is available, use it only for bincode (native)
        // keys. Compact postcard keys (starting with OVH2) always use the portable
        // verifier so the same key format works on all targets.
        #[cfg(feature = "halo2-std")]
        if !is_portable_halo2_key_payload(&key.payload) {
            halo2_native::verify(key, proof)?;
            return Ok(VerificationReport::new(
                proof.proof_kind,
                true,
                proof.user_public_values.len(),
            ));
        }

        let _report = halo2_portable::verify_frontend(key, proof, self.on_step)?;
        Ok(VerificationReport::new(
            ProofKind::OpenVmEvmHalo2,
            true,
            proof.user_public_values.len(),
        ))
    }
}

#[cfg(feature = "halo2-std")]
pub fn compact_halo2_key_from_native_payload(payload: &[u8]) -> Result<Vec<u8>, VerifyError> {
    halo2_native::compact_key_from_native_payload(payload)
}

#[cfg(feature = "halo2-std")]
pub fn debug_compact_halo2_key_from_native_payload(
    payload: &[u8],
) -> (&'static str, Result<Vec<u8>, VerifyError>) {
    halo2_native::debug_compact_key_from_native_payload(payload)
}

#[cfg(feature = "halo2-std")]
mod halo2_native {
    use alloc::vec::Vec;
    use std::io::Cursor;

    use serde::{Deserialize, Serialize};
    use snark_verifier_sdk::{
        snark_verifier::{
            halo2_base::halo2_proofs::halo2curves::{
                bn256::{Bn256, Fr, G1Affine, G2Affine},
                ff::PrimeField,
                serde::SerdeObject,
            },
            loader::native::NativeLoader,
            pcs::kzg::KzgDecidingKey,
            system::halo2::transcript::evm::EvmTranscript,
            verifier::{plonk::PlonkProtocol, SnarkVerifier},
        },
        PlonkVerifier, SHPLONK,
    };

    use super::{
        encode_portable_halo2_key, halo2_portable::convert_native_protocol, EncodedG1,
        EncodedG2, EncodedScalar, PortableHalo2VerifierKey, ProofEnvelope, VerifierKey,
        VerifyError,
    };

    const BN254_BYTES: usize = 32;
    const NUM_ACCUMULATOR: usize = 12;

    #[derive(Debug, Serialize, Deserialize)]
    struct NativeHalo2VerifierKey {
        protocol: PlonkProtocol<G1Affine>,
        deciding_key: KzgDecidingKey<Bn256>,
        num_pvs: Vec<usize>,
        k: usize,
    }

    pub fn verify(key: &VerifierKey, proof: &ProofEnvelope) -> Result<(), VerifyError> {
        let native_key: NativeHalo2VerifierKey =
            bincode::deserialize(&key.payload).map_err(|_| VerifyError::InvalidHalo2VerifierKey)?;
        let instances = vec![decode_instances(proof)?];
        if native_key.num_pvs != instances.iter().map(Vec::len).collect::<Vec<_>>() {
            return Err(VerifyError::InvalidHalo2VerifierKey);
        }

        let proof_bytes = proof.proof_data[NUM_ACCUMULATOR * BN254_BYTES..].to_vec();
        let mut transcript =
            EvmTranscript::<G1Affine, NativeLoader, _, _>::new(Cursor::new(proof_bytes));
        let plonk_proof = PlonkVerifier::<SHPLONK>::read_proof(
            &native_key.deciding_key,
            &native_key.protocol,
            &instances,
            &mut transcript,
        )
        .map_err(|_| VerifyError::Halo2VerificationFailed)?;
        PlonkVerifier::<SHPLONK>::verify(
            &native_key.deciding_key,
            &native_key.protocol,
            &instances,
            &plonk_proof,
        )
        .map_err(|_| VerifyError::Halo2VerificationFailed)
    }

    pub fn compact_key_from_native_payload(payload: &[u8]) -> Result<Vec<u8>, VerifyError> {
        debug_compact_key_from_native_payload(payload).1
    }

    pub fn debug_compact_key_from_native_payload(
        payload: &[u8],
    ) -> (&'static str, Result<Vec<u8>, VerifyError>) {
        let native_key: NativeHalo2VerifierKey =
            match bincode::deserialize(payload) {
                Ok(key) => key,
                Err(_) => {
                    return ("native key decode", Err(VerifyError::InvalidHalo2VerifierKey));
                }
            };
        let mut compact = PortableHalo2VerifierKey::new(
            native_key.k as u32,
            match encode_g1(&native_key.deciding_key.svk().g) {
                Ok(value) => value,
                Err(error) => return ("svk g encode", Err(error)),
            },
            match encode_g2(&native_key.deciding_key.g2()) {
                Ok(value) => value,
                Err(error) => return ("g2 encode", Err(error)),
            },
            match encode_g2(&native_key.deciding_key.s_g2()) {
                Ok(value) => value,
                Err(error) => return ("s_g2 encode", Err(error)),
            },
        );
        compact.domain_k = native_key.protocol.domain.k as u32;
        compact.num_instance = match to_u32_vec(&native_key.num_pvs) {
            Ok(values) => values,
            Err(error) => return ("num_instance", Err(error)),
        };
        compact.num_witness = match to_u32_vec(&native_key.protocol.num_witness) {
            Ok(values) => values,
            Err(error) => return ("num_witness", Err(error)),
        };
        compact.num_challenge = match to_u32_vec(&native_key.protocol.num_challenge) {
            Ok(values) => values,
            Err(error) => return ("num_challenge", Err(error)),
        };
        compact.evaluations_len = native_key.protocol.evaluations.len() as u32;
        compact.queries_len = native_key.protocol.queries.len() as u32;
        compact.accumulator_indices_len = native_key
            .protocol
            .accumulator_indices
            .iter()
            .map(|indices| indices.len() as u32)
            .collect();
        compact.transcript_initial_state = match native_key
            .protocol
            .transcript_initial_state
            .as_ref()
            .map(encode_scalar_be)
            .transpose()
        {
            Ok(value) => value,
            Err(error) => return ("transcript state", Err(error)),
        };
        compact.preprocessed = match native_key
            .protocol
            .preprocessed
            .iter()
            .map(encode_g1)
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(values) => values,
            Err(error) => return ("preprocessed encode", Err(error)),
        };
        let portable_protocol = match convert_native_protocol(&native_key.protocol) {
            Ok(protocol) => protocol,
            Err(error) => return ("protocol convert", Err(error)),
        };
        compact.protocol = match postcard::to_allocvec(&portable_protocol) {
            Ok(bytes) => bytes,
            Err(_) => return ("protocol encode", Err(VerifyError::InvalidHalo2VerifierKey)),
        };
        (
            "portable key encode",
            encode_portable_halo2_key(&compact).map_err(|_| VerifyError::InvalidHalo2VerifierKey),
        )
    }

    fn to_u32_vec(values: &[usize]) -> Result<Vec<u32>, VerifyError> {
        values
            .iter()
            .map(|value| u32::try_from(*value).map_err(|_| VerifyError::InvalidHalo2VerifierKey))
            .collect()
    }

    fn encode_scalar_be(scalar: &Fr) -> Result<EncodedScalar, VerifyError> {
        let mut bytes = scalar.to_repr();
        bytes.as_mut().reverse();
        let mut encoded = [0; BN254_BYTES];
        encoded.copy_from_slice(bytes.as_ref());
        Ok(EncodedScalar(encoded))
    }

    fn encode_g1(point: &G1Affine) -> Result<EncodedG1, VerifyError> {
        let bytes = point.to_raw_bytes();
        let mut encoded = [0; 64];
        if bytes.len() != encoded.len() {
            return Err(VerifyError::InvalidHalo2VerifierKey);
        }
        encoded.copy_from_slice(&bytes);
        Ok(EncodedG1(encoded.to_vec()))
    }

    fn encode_g2(point: &G2Affine) -> Result<EncodedG2, VerifyError> {
        let bytes = point.to_raw_bytes();
        let mut encoded = [0; 128];
        if bytes.len() != encoded.len() {
            return Err(VerifyError::InvalidHalo2VerifierKey);
        }
        encoded.copy_from_slice(&bytes);
        Ok(EncodedG2(encoded.to_vec()))
    }

    fn decode_instances(proof: &ProofEnvelope) -> Result<Vec<Fr>, VerifyError> {
        let mut words = Vec::with_capacity(NUM_ACCUMULATOR + 2 + proof.user_public_values.len());

        for chunk in proof.proof_data[..NUM_ACCUMULATOR * BN254_BYTES].chunks_exact(BN254_BYTES) {
            let mut word = [0; BN254_BYTES];
            word.copy_from_slice(chunk);
            word.reverse();
            words.push(word);
        }

        let mut app_exe_commit = proof.app_exe_commit;
        app_exe_commit.reverse();
        words.push(app_exe_commit);

        let mut app_vm_commit = proof.app_vm_commit;
        app_vm_commit.reverse();
        words.push(app_vm_commit);

        for byte in &proof.user_public_values {
            let mut word = [0; BN254_BYTES];
            word[0] = *byte;
            words.push(word);
        }

        words
            .iter()
            .map(|word| {
                Option::<Fr>::from(Fr::from_bytes(word))
                    .ok_or(VerifyError::InvalidHalo2FieldElement)
            })
            .collect()
    }
}

#[derive(Default)]
pub struct OpenVmStarkVerifier;

impl Verifier for OpenVmStarkVerifier {
    fn verify(
        &mut self,
        key: &VerifierKey,
        proof: &ProofEnvelope,
    ) -> Result<VerificationReport, VerifyError> {
        validate_common_inputs(key, proof)?;
        if proof.proof_kind != ProofKind::OpenVmStark {
            return Err(VerifyError::ProofKindMismatch);
        }
        if proof.metadata.is_empty() {
            return Err(VerifyError::MissingStarkBaseline);
        }

        #[cfg(feature = "stark-std")]
        {
            let proof_json =
                core::str::from_utf8(&proof.proof_data).map_err(|_| VerifyError::InvalidUtf8)?;
            let baseline_json =
                core::str::from_utf8(&proof.metadata).map_err(|_| VerifyError::InvalidUtf8)?;
            openvm_wasm_verifier::verify_stark_native(proof_json, &key.payload, baseline_json)
                .map_err(|_| VerifyError::StarkVerificationFailed)?;
            Ok(VerificationReport::new(
                proof.proof_kind,
                true,
                proof.user_public_values.len(),
            ))
        }

        #[cfg(not(feature = "stark-std"))]
        {
            let _ = key;
            let _ = proof;
            Err(VerifyError::CryptoBackendUnavailable)
        }
    }
}

#[cfg(any(test, feature = "structural-only"))]
#[derive(Default)]
pub struct StructuralOnlyVerifier;

#[cfg(any(test, feature = "structural-only"))]
impl Verifier for StructuralOnlyVerifier {
    fn verify(
        &mut self,
        key: &VerifierKey,
        proof: &ProofEnvelope,
    ) -> Result<VerificationReport, VerifyError> {
        validate_evm_halo2_inputs(key, proof)?;
        Ok(VerificationReport::new(
            proof.proof_kind,
            true,
            proof.user_public_values.len(),
        ))
    }
}

pub fn validate_evm_halo2_inputs(
    key: &VerifierKey,
    proof: &ProofEnvelope,
) -> Result<(), VerifyError> {
    validate_common_inputs(key, proof)?;

    match proof.proof_kind {
        ProofKind::OpenVmEvmHalo2 => {
            if proof.proof_data.len() != OPENVM_EVM_HALO2_PROOF_DATA_LEN {
                return Err(VerifyError::InvalidEvmHalo2ProofDataLen {
                    expected: OPENVM_EVM_HALO2_PROOF_DATA_LEN,
                    actual: proof.proof_data.len(),
                });
            }
        }
        ProofKind::OpenVmStark => return Err(VerifyError::CryptoBackendUnavailable),
    }

    Ok(())
}

fn validate_common_inputs(key: &VerifierKey, proof: &ProofEnvelope) -> Result<(), VerifyError> {
    ensure_protocol_version(key.protocol_version)?;
    ensure_protocol_version(proof.protocol_version)?;

    if key.proof_kind != proof.proof_kind {
        return Err(VerifyError::ProofKindMismatch);
    }
    if proof.verifier_key_id != key.key_id {
        return Err(VerifyError::VerifierKeyMismatch);
    }
    if key.payload.is_empty() {
        return Err(VerifyError::EmptyVerifierKey);
    }
    if proof.user_public_values.is_empty() {
        return Err(VerifyError::EmptyPublicValues);
    }
    if proof.app_exe_commit == [0; 32] || proof.app_vm_commit == [0; 32] {
        return Err(VerifyError::EmptyCommitment);
    }
    Ok(())
}

fn ensure_protocol_version(actual: u16) -> Result<(), VerifyError> {
    if actual == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(VerifyError::UnsupportedProtocolVersion {
            expected: PROTOCOL_VERSION,
            actual,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VerifierKey;

    fn proof_and_key() -> (ProofEnvelope, VerifierKey) {
        let key = VerifierKey::new(ProofKind::OpenVmEvmHalo2, [9; 32], vec![1, 2, 3]);
        let proof = ProofEnvelope::new_evm_halo2(
            "v2.0".into(),
            key.key_id,
            [1; 32],
            [2; 32],
            vec![42],
            vec![7; OPENVM_EVM_HALO2_PROOF_DATA_LEN],
        );
        (proof, key)
    }

    #[test]
    fn production_verifier_fails_closed() {
        let (proof, key) = proof_and_key();
        let mut verifier = OpenVmEvmHalo2Verifier::default();
        #[cfg(feature = "std")]
        assert_ne!(
            verifier.verify(&key, &proof),
            Ok(VerificationReport::new(ProofKind::OpenVmEvmHalo2, true, 1))
        );
        #[cfg(not(feature = "std"))]
        assert_eq!(
            verifier.verify(&key, &proof).unwrap_err(),
            VerifyError::CryptoBackendUnavailable
        );
    }

    #[test]
    fn structural_verifier_accepts_well_formed_fixture() {
        let (proof, key) = proof_and_key();
        let mut verifier = StructuralOnlyVerifier;
        let report = verifier.verify(&key, &proof).unwrap();
        assert!(report.verified);
        assert_eq!(report.public_values_len, 1);
    }
}
