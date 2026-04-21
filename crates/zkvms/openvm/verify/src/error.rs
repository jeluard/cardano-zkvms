use openvm_stark_backend::verifier::VerifierError;
use openvm_stark_sdk::config::baby_bear_poseidon2::{EF, F};
use thiserror::Error;

use crate::{public_values::UserPublicValuesProofError, types::CommitBytes};

#[derive(Error, Debug)]
pub enum VerifyStarkError {
    #[error("STARK verifier failed: {0}")]
    StarkVerificationFailure(#[from] VerifierError<EF>),
    #[error("User public values verification failed: {0}")]
    UserPvsVerificationFailure(#[from] UserPublicValuesProofError),
    #[error("Deferred proofs are not supported by the browser verifier")]
    UnsupportedDeferrals,
    #[error("Missing public values for air {air_idx}")]
    MissingPublicValues { air_idx: usize },
    #[error("Verifier public values too short: expected at least {expected}, actual {actual}")]
    InvalidVerifierPvsLength { expected: usize, actual: usize },
    #[error("VM public values too short: expected at least {expected}, actual {actual}")]
    InvalidVmPvsLength { expected: usize, actual: usize },
    #[error("Invalid internal proof flag: {0:?} (expected 2 for final internal-recursive proof)")]
    InvalidInternalFlag(F),
    #[error("Invalid recursion flag: {0:?} (expected 1 or 2)")]
    InvalidRecursionFlag(F),
    #[error("Commit mismatch for {kind}.{field}: expected {expected}, actual {actual}")]
    CommitMismatch {
        kind: &'static str,
        field: &'static str,
        expected: CommitBytes,
        actual: CommitBytes,
    },
    #[error("Missing trace verification data for air {air_idx}")]
    MissingTraceVerificationData { air_idx: usize },
    #[error("Missing cached commitment {cached_idx} for air {air_idx}")]
    MissingCachedCommitment { air_idx: usize, cached_idx: usize },
    #[error("Execution did not terminate successfully: exit_code={exit_code:?} is_terminate={is_terminate:?}")]
    ExecutionDidNotSucceed { exit_code: F, is_terminate: F },
}