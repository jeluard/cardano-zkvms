use openvm_stark_backend::{
    keygen::types::MultiStarkVerifyingKey, p3_field::PrimeCharacteristicRing, StarkEngine,
};
use openvm_stark_sdk::config::baby_bear_poseidon2::{
    BabyBearPoseidon2Config as SC, BabyBearPoseidon2CpuEngine, DuplexSponge, DIGEST_SIZE, F,
};

use crate::{
    error::VerifyStarkError,
    hasher::{vm_poseidon2_hasher, Hasher},
    types::{CommitBytes, Digest, VerificationBaseline, VkCommit, VmStarkProof},
};

const VERIFIER_PVS_AIR_ID: usize = 0;
const VM_PVS_AIR_ID: usize = 1;
const DEF_PVS_AIR_ID: usize = 2;
const CONSTRAINT_EVAL_AIR_ID: usize = 3;
const CONSTRAINT_EVAL_CACHED_INDEX: usize = 0;

const VK_COMMIT_LEN: usize = DIGEST_SIZE * 2;
const VERIFIER_BASE_PVS_LEN: usize = 2 + VK_COMMIT_LEN * 4;
const VM_PVS_LEN: usize = DIGEST_SIZE * 3 + 4;

#[derive(Clone, Copy, Debug)]
struct VerifierBasePvs<T> {
    internal_flag: T,
    app_vk_commit: VkCommit<T>,
    leaf_vk_commit: VkCommit<T>,
    internal_for_leaf_vk_commit: VkCommit<T>,
    recursion_flag: T,
    internal_recursive_vk_commit: VkCommit<T>,
}

#[derive(Clone, Copy, Debug)]
struct VmPvs<T> {
    program_commit: [T; DIGEST_SIZE],
    initial_pc: T,
    exit_code: T,
    is_terminate: T,
    initial_memory_root: [T; DIGEST_SIZE],
    final_memory_root: [T; DIGEST_SIZE],
}

pub fn verify_vm_stark_proof_decoded(
    agg_vk: &MultiStarkVerifyingKey<SC>,
    baseline: &VerificationBaseline,
    proof: &VmStarkProof,
) -> Result<(), VerifyStarkError> {
    if baseline.expected_def_hook_commit.is_some()
        || proof.deferral_merkle_proofs.is_some()
        || proof
            .inner
            .public_values
            .get(DEF_PVS_AIR_ID)
            .is_some_and(|values| !values.is_empty())
    {
        return Err(VerifyStarkError::UnsupportedDeferrals);
    }

    let engine = BabyBearPoseidon2CpuEngine::<DuplexSponge>::new(agg_vk.inner.params.clone());
    engine.verify(agg_vk, &proof.inner)?;
    verify_vm_stark_proof_pvs(baseline, proof)
}

fn verify_vm_stark_proof_pvs(
    baseline: &VerificationBaseline,
    proof: &VmStarkProof,
) -> Result<(), VerifyStarkError> {
    let verifier_pvs = proof.inner.public_values.get(VERIFIER_PVS_AIR_ID).ok_or(
        VerifyStarkError::MissingPublicValues {
            air_idx: VERIFIER_PVS_AIR_ID,
        },
    )?;
    if verifier_pvs.len() < VERIFIER_BASE_PVS_LEN {
        return Err(VerifyStarkError::InvalidVerifierPvsLength {
            expected: VERIFIER_BASE_PVS_LEN,
            actual: verifier_pvs.len(),
        });
    }

    let verifier_base_pvs = parse_verifier_base_pvs(&verifier_pvs[..VERIFIER_BASE_PVS_LEN]);
    if verifier_base_pvs.internal_flag != F::TWO {
        return Err(VerifyStarkError::InvalidInternalFlag(
            verifier_base_pvs.internal_flag,
        ));
    }
    if verifier_base_pvs.recursion_flag != F::ONE && verifier_base_pvs.recursion_flag != F::TWO {
        return Err(VerifyStarkError::InvalidRecursionFlag(
            verifier_base_pvs.recursion_flag,
        ));
    }

    ensure_commit_eq(
        "app_vk_commit",
        "cached_commit",
        &baseline.app_vk_commit.cached_commit,
        &verifier_base_pvs.app_vk_commit.cached_commit,
    )?;
    ensure_commit_eq(
        "app_vk_commit",
        "vk_pre_hash",
        &baseline.app_vk_commit.vk_pre_hash,
        &verifier_base_pvs.app_vk_commit.vk_pre_hash,
    )?;
    ensure_commit_eq(
        "leaf_vk_commit",
        "cached_commit",
        &baseline.leaf_vk_commit.cached_commit,
        &verifier_base_pvs.leaf_vk_commit.cached_commit,
    )?;
    ensure_commit_eq(
        "leaf_vk_commit",
        "vk_pre_hash",
        &baseline.leaf_vk_commit.vk_pre_hash,
        &verifier_base_pvs.leaf_vk_commit.vk_pre_hash,
    )?;
    ensure_commit_eq(
        "internal_for_leaf_vk_commit",
        "cached_commit",
        &baseline.internal_for_leaf_vk_commit.cached_commit,
        &verifier_base_pvs.internal_for_leaf_vk_commit.cached_commit,
    )?;
    ensure_commit_eq(
        "internal_for_leaf_vk_commit",
        "vk_pre_hash",
        &baseline.internal_for_leaf_vk_commit.vk_pre_hash,
        &verifier_base_pvs.internal_for_leaf_vk_commit.vk_pre_hash,
    )?;

    let vm_pvs = proof.inner.public_values.get(VM_PVS_AIR_ID).ok_or(
        VerifyStarkError::MissingPublicValues {
            air_idx: VM_PVS_AIR_ID,
        },
    )?;
    if vm_pvs.len() < VM_PVS_LEN {
        return Err(VerifyStarkError::InvalidVmPvsLength {
            expected: VM_PVS_LEN,
            actual: vm_pvs.len(),
        });
    }

    let vm_pvs = parse_vm_pvs(&vm_pvs[..VM_PVS_LEN]);
    let hasher = vm_poseidon2_hasher();
    proof.user_pvs_proof.verify(
        &hasher,
        baseline.memory_dimensions,
        vm_pvs.final_memory_root,
    )?;

    let computed_app_exe_commit = compute_exe_commit(
        &hasher,
        &vm_pvs.program_commit,
        &vm_pvs.initial_memory_root,
        vm_pvs.initial_pc,
    );
    ensure_commit_eq(
        "app_exe",
        "commit",
        &baseline.app_exe_commit,
        &computed_app_exe_commit,
    )?;

    if vm_pvs.exit_code != F::ZERO || vm_pvs.is_terminate != F::ONE {
        return Err(VerifyStarkError::ExecutionDidNotSucceed {
            exit_code: vm_pvs.exit_code,
            is_terminate: vm_pvs.is_terminate,
        });
    }

    let trace_vdata = proof
        .inner
        .trace_vdata
        .get(CONSTRAINT_EVAL_AIR_ID)
        .ok_or(VerifyStarkError::MissingTraceVerificationData {
            air_idx: CONSTRAINT_EVAL_AIR_ID,
        })?
        .as_ref()
        .ok_or(VerifyStarkError::MissingTraceVerificationData {
            air_idx: CONSTRAINT_EVAL_AIR_ID,
        })?;
    let proof_cached_commit = trace_vdata
        .cached_commitments
        .get(CONSTRAINT_EVAL_CACHED_INDEX)
        .cloned()
        .ok_or(VerifyStarkError::MissingCachedCommitment {
            air_idx: CONSTRAINT_EVAL_AIR_ID,
            cached_idx: CONSTRAINT_EVAL_CACHED_INDEX,
        })?
        .into();
    if verifier_base_pvs.recursion_flag == F::TWO {
        ensure_commit_eq(
            "internal_recursive_vk_commit",
            "cached_commit",
            &baseline.internal_recursive_vk_commit.cached_commit,
            &verifier_base_pvs.internal_recursive_vk_commit.cached_commit,
        )?;
        ensure_commit_eq(
            "internal_recursive_vk_commit",
            "vk_pre_hash",
            &baseline.internal_recursive_vk_commit.vk_pre_hash,
            &verifier_base_pvs.internal_recursive_vk_commit.vk_pre_hash,
        )?;
        ensure_commit_eq(
            "constraint_eval_trace",
            "cached_commit",
            &baseline.internal_recursive_vk_commit.cached_commit,
            &proof_cached_commit,
        )?;
    } else {
        ensure_commit_unset(
            "internal_recursive_vk_commit",
            "cached_commit",
            &verifier_base_pvs.internal_recursive_vk_commit.cached_commit,
        )?;
        ensure_commit_unset(
            "internal_recursive_vk_commit",
            "vk_pre_hash",
            &verifier_base_pvs.internal_recursive_vk_commit.vk_pre_hash,
        )?;
        ensure_commit_eq(
            "constraint_eval_trace",
            "cached_commit",
            &baseline.internal_for_leaf_vk_commit.cached_commit,
            &proof_cached_commit,
        )?;
    }

    Ok(())
}

fn compute_exe_commit(
    hasher: &impl Hasher<DIGEST_SIZE, F>,
    program_commit: &Digest,
    init_memory_root: &Digest,
    pc_start: F,
) -> Digest {
    let mut padded_pc_start = [F::ZERO; DIGEST_SIZE];
    padded_pc_start[0] = pc_start;

    let program_hash = hasher.hash(program_commit);
    let memory_hash = hasher.hash(init_memory_root);
    let pc_hash = hasher.hash(&padded_pc_start);

    hasher.compress(&hasher.compress(&program_hash, &memory_hash), &pc_hash)
}

fn parse_verifier_base_pvs(values: &[F]) -> VerifierBasePvs<F> {
    let mut offset = 0;
    let internal_flag = values[offset];
    offset += 1;
    let app_vk_commit = parse_vk_commit(&values[offset..offset + VK_COMMIT_LEN]);
    offset += VK_COMMIT_LEN;
    let leaf_vk_commit = parse_vk_commit(&values[offset..offset + VK_COMMIT_LEN]);
    offset += VK_COMMIT_LEN;
    let internal_for_leaf_vk_commit = parse_vk_commit(&values[offset..offset + VK_COMMIT_LEN]);
    offset += VK_COMMIT_LEN;
    let recursion_flag = values[offset];
    offset += 1;
    let internal_recursive_vk_commit = parse_vk_commit(&values[offset..offset + VK_COMMIT_LEN]);

    VerifierBasePvs {
        internal_flag,
        app_vk_commit,
        leaf_vk_commit,
        internal_for_leaf_vk_commit,
        recursion_flag,
        internal_recursive_vk_commit,
    }
}

fn parse_vm_pvs(values: &[F]) -> VmPvs<F> {
    let mut offset = 0;
    let program_commit = parse_digest(&values[offset..offset + DIGEST_SIZE]);
    offset += DIGEST_SIZE;
    let initial_pc = values[offset];
    offset += 1;
    let _final_pc = values[offset];
    offset += 1;
    let exit_code = values[offset];
    offset += 1;
    let is_terminate = values[offset];
    offset += 1;
    let initial_memory_root = parse_digest(&values[offset..offset + DIGEST_SIZE]);
    offset += DIGEST_SIZE;
    let final_memory_root = parse_digest(&values[offset..offset + DIGEST_SIZE]);

    VmPvs {
        program_commit,
        initial_pc,
        exit_code,
        is_terminate,
        initial_memory_root,
        final_memory_root,
    }
}

fn parse_vk_commit(values: &[F]) -> VkCommit<F> {
    VkCommit {
        cached_commit: parse_digest(&values[..DIGEST_SIZE]),
        vk_pre_hash: parse_digest(&values[DIGEST_SIZE..DIGEST_SIZE * 2]),
    }
}

fn parse_digest(values: &[F]) -> Digest {
    values.try_into().expect("validated digest width")
}

fn ensure_commit_eq(
    kind: &'static str,
    field: &'static str,
    expected: &Digest,
    actual: &Digest,
) -> Result<(), VerifyStarkError> {
    if expected == actual {
        Ok(())
    } else {
        Err(VerifyStarkError::CommitMismatch {
            kind,
            field,
            expected: CommitBytes::from_digest(expected),
            actual: CommitBytes::from_digest(actual),
        })
    }
}

fn ensure_commit_unset(
    kind: &'static str,
    field: &'static str,
    actual: &Digest,
) -> Result<(), VerifyStarkError> {
    let expected = [F::ZERO; DIGEST_SIZE];
    ensure_commit_eq(kind, field, &expected, actual)
}
