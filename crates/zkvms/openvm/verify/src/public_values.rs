use std::io;

use openvm_stark_backend::{codec::DecodableConfig, p3_util::log2_strict_usize};
use openvm_stark_sdk::config::baby_bear_poseidon2::F;
use thiserror::Error;

use crate::{hasher::Hasher, types::MemoryDimensions};

pub const PUBLIC_VALUES_AS: u32 = 3;

#[derive(Clone, Debug)]
pub struct UserPublicValuesProof<const CHUNK: usize, T> {
    pub proof: Vec<[T; CHUNK]>,
    pub public_values: Vec<T>,
    pub public_values_commit: [T; CHUNK],
}

#[derive(Error, Debug)]
pub enum UserPublicValuesProofError {
    #[error("unexpected public values length: {0}")]
    UnexpectedLength(usize),
    #[error("incorrect proof length: {0} (expected {1})")]
    IncorrectProofLength(usize, usize),
    #[error("user public values do not match commitment")]
    UserPublicValuesCommitMismatch,
    #[error("final memory root mismatch")]
    FinalMemoryRootMismatch,
}

impl<const CHUNK: usize> UserPublicValuesProof<CHUNK, F> {
    pub fn verify(
        &self,
        hasher: &impl Hasher<CHUNK, F>,
        memory_dimensions: MemoryDimensions,
        final_memory_root: [F; CHUNK],
    ) -> Result<(), UserPublicValuesProofError> {
        let pvs = &self.public_values;
        if pvs.len() % CHUNK != 0 || !(pvs.len() / CHUNK).is_power_of_two() {
            return Err(UserPublicValuesProofError::UnexpectedLength(pvs.len()));
        }

        let pv_height = log2_strict_usize(pvs.len() / CHUNK);
        let proof_len = memory_dimensions.overall_height() - pv_height;
        let idx_prefix = memory_dimensions.label_to_index((PUBLIC_VALUES_AS, 0)) >> pv_height;

        if self.proof.len() != proof_len {
            return Err(UserPublicValuesProofError::IncorrectProofLength(
                self.proof.len(),
                proof_len,
            ));
        }

        let mut curr_root = self.public_values_commit;
        for (i, sibling_hash) in self.proof.iter().enumerate() {
            curr_root = if idx_prefix & (1 << i) != 0 {
                hasher.compress(sibling_hash, &curr_root)
            } else {
                hasher.compress(&curr_root, sibling_hash)
            };
        }

        if curr_root != final_memory_root {
            return Err(UserPublicValuesProofError::FinalMemoryRootMismatch);
        }

        if hasher.merkle_root(pvs) != self.public_values_commit {
            return Err(UserPublicValuesProofError::UserPublicValuesCommitMismatch);
        }

        Ok(())
    }

    pub fn decode<SC: DecodableConfig<F = F, Digest = [F; CHUNK]>, R: io::Read>(
        reader: &mut R,
    ) -> io::Result<Self> {
        let proof = SC::decode_digest_vec(reader)?;
        let public_values = SC::decode_base_field_vec(reader)?;
        let public_values_commit = SC::decode_digest(reader)?;
        Ok(Self {
            proof,
            public_values,
            public_values_commit,
        })
    }
}
