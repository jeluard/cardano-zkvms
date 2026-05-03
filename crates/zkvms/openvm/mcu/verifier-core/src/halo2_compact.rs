use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::VerifyError;

pub const PORTABLE_HALO2_KEY_MAGIC: [u8; 4] = *b"OVH2";
pub const PORTABLE_HALO2_KEY_FORMAT_VERSION: u16 = 2;
pub const BN254_SCALAR_BYTES: usize = 32;
pub const BN254_G1_RAW_BYTES: usize = 64;
pub const BN254_G2_RAW_BYTES: usize = 128;
pub const OPENVM_EVM_HALO2_ACCUMULATOR_WORDS: u16 = 12;
pub const OPENVM_EVM_HALO2_PROOF_WORDS: u16 = 43;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EncodedScalar(pub [u8; BN254_SCALAR_BYTES]);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EncodedG1(pub Vec<u8>);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EncodedG2(pub Vec<u8>);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Halo2ProofShape {
    pub accumulator_words: u16,
    pub proof_words: u16,
}

impl Default for Halo2ProofShape {
    fn default() -> Self {
        Self {
            accumulator_words: OPENVM_EVM_HALO2_ACCUMULATOR_WORDS,
            proof_words: OPENVM_EVM_HALO2_PROOF_WORDS,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PortableHalo2VerifierKey {
    pub magic: [u8; 4],
    pub format_version: u16,
    pub k: u32,
    pub domain_k: u32,
    pub num_instance: Vec<u32>,
    pub num_witness: Vec<u32>,
    pub num_challenge: Vec<u32>,
    pub evaluations_len: u32,
    pub queries_len: u32,
    pub accumulator_indices_len: Vec<u32>,
    pub transcript_initial_state: Option<EncodedScalar>,
    pub preprocessed: Vec<EncodedG1>,
    pub protocol: Vec<u8>,
    pub g1: EncodedG1,
    pub g2: EncodedG2,
    pub s_g2: EncodedG2,
    pub proof_shape: Halo2ProofShape,
}

impl PortableHalo2VerifierKey {
    pub fn new(k: u32, g1: EncodedG1, g2: EncodedG2, s_g2: EncodedG2) -> Self {
        Self {
            magic: PORTABLE_HALO2_KEY_MAGIC,
            format_version: PORTABLE_HALO2_KEY_FORMAT_VERSION,
            k,
            domain_k: k,
            num_instance: Vec::new(),
            num_witness: Vec::new(),
            num_challenge: Vec::new(),
            evaluations_len: 0,
            queries_len: 0,
            accumulator_indices_len: Vec::new(),
            transcript_initial_state: None,
            preprocessed: Vec::new(),
            protocol: Vec::new(),
            g1,
            g2,
            s_g2,
            proof_shape: Halo2ProofShape::default(),
        }
    }

    pub fn validate_header(&self) -> Result<(), VerifyError> {
        if self.magic != PORTABLE_HALO2_KEY_MAGIC
            || self.format_version != PORTABLE_HALO2_KEY_FORMAT_VERSION
        {
            return Err(VerifyError::InvalidHalo2VerifierKey);
        }
        Ok(())
    }
}

pub fn is_portable_halo2_key_payload(payload: &[u8]) -> bool {
    payload.starts_with(&PORTABLE_HALO2_KEY_MAGIC)
}

pub fn encode_portable_halo2_key(
    key: &PortableHalo2VerifierKey,
) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(key)
}

pub fn decode_portable_halo2_key(payload: &[u8]) -> Result<PortableHalo2VerifierKey, VerifyError> {
    let key: PortableHalo2VerifierKey =
        postcard::from_bytes(payload).map_err(|_| VerifyError::InvalidHalo2VerifierKey)?;
    key.validate_header()?;
    Ok(key)
}
