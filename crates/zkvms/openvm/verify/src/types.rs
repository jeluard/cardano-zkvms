use std::{fmt, io::Cursor};

use num_bigint::BigUint;
use openvm_stark_backend::{
    codec::Decode,
    p3_field::{PrimeCharacteristicRing, PrimeField32},
    proof::Proof,
};
use openvm_stark_sdk::{
    config::baby_bear_poseidon2::{BabyBearPoseidon2Config as SC, DIGEST_SIZE, F},
};
use serde::{de, Deserialize, Deserializer};
use serde_with::serde_as;

use crate::public_values::UserPublicValuesProof;

pub const BN254_BYTES: usize = 32;
pub const ADDR_SPACE_OFFSET: u32 = 1;

pub type Digest = [F; DIGEST_SIZE];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct MemoryDimensions {
    pub addr_space_height: usize,
    pub address_height: usize,
}

impl MemoryDimensions {
    pub fn overall_height(&self) -> usize {
        self.addr_space_height + self.address_height
    }

    pub fn label_to_index(&self, (addr_space, block_id): (u32, u32)) -> u64 {
        (((addr_space - ADDR_SPACE_OFFSET) as u64) << self.address_height) + block_id as u64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommitBytes(pub [u8; BN254_BYTES]);

impl CommitBytes {
    pub fn to_u32_digest(&self) -> [u32; DIGEST_SIZE] {
        let mut bigint = BigUint::default();
        for byte in self.0 {
            bigint <<= 8usize;
            bigint += BigUint::from(byte);
        }

        let order = BigUint::from(F::ORDER_U32);
        std::array::from_fn(|_| {
            let digits = (&bigint % &order).to_u32_digits();
            let digit = digits.first().copied().unwrap_or(0);
            bigint /= &order;
            digit
        })
    }

    pub fn to_digest(&self) -> Digest {
        self.to_u32_digest().map(F::from_u32)
    }

    pub fn from_digest(digest: &Digest) -> Self {
        let order = BigUint::from(F::ORDER_U32);
        let mut bigint = BigUint::default();

        for value in digest.iter().rev() {
            bigint *= &order;
            bigint += BigUint::from(value.as_canonical_u32());
        }

        let encoded = bigint.to_bytes_be();
        let mut bytes = [0u8; BN254_BYTES];
        let offset = BN254_BYTES.saturating_sub(encoded.len());
        bytes[offset..offset + encoded.len()].copy_from_slice(&encoded);
        Self(bytes)
    }
}

impl fmt::Display for CommitBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for CommitBytes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let hex = value
            .strip_prefix("0x")
            .ok_or_else(|| de::Error::custom("expected 0x-prefixed hex"))?;
        let bytes = hex::decode(hex).map_err(de::Error::custom)?;
        let bytes: [u8; BN254_BYTES] = bytes
            .try_into()
            .map_err(|_| de::Error::custom("expected 32-byte commitment"))?;
        Ok(Self(bytes))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VkCommit<T> {
    pub cached_commit: [T; DIGEST_SIZE],
    pub vk_pre_hash: [T; DIGEST_SIZE],
}

#[derive(Clone, Debug, Deserialize)]
pub struct VkCommitJson {
    pub cached_commit: CommitBytes,
    pub vk_pre_hash: CommitBytes,
}

impl VkCommitJson {
    pub fn to_vk_commit(&self) -> VkCommit<F> {
        VkCommit {
            cached_commit: self.cached_commit.to_digest(),
            vk_pre_hash: self.vk_pre_hash.to_digest(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct VerificationBaselineJson {
    pub app_exe_commit: CommitBytes,
    pub memory_dimensions: MemoryDimensions,
    pub app_vk_commit: VkCommitJson,
    pub leaf_vk_commit: VkCommitJson,
    pub internal_for_leaf_vk_commit: VkCommitJson,
    pub internal_recursive_vk_commit: VkCommitJson,
    pub expected_def_hook_commit: Option<CommitBytes>,
}

#[derive(Clone, Debug)]
pub struct VerificationBaseline {
    pub app_exe_commit: Digest,
    pub memory_dimensions: MemoryDimensions,
    pub app_vk_commit: VkCommit<F>,
    pub leaf_vk_commit: VkCommit<F>,
    pub internal_for_leaf_vk_commit: VkCommit<F>,
    pub internal_recursive_vk_commit: VkCommit<F>,
    pub expected_def_hook_commit: Option<Digest>,
}

impl From<VerificationBaselineJson> for VerificationBaseline {
    fn from(value: VerificationBaselineJson) -> Self {
        Self {
            app_exe_commit: value.app_exe_commit.to_digest(),
            memory_dimensions: value.memory_dimensions,
            app_vk_commit: value.app_vk_commit.to_vk_commit(),
            leaf_vk_commit: value.leaf_vk_commit.to_vk_commit(),
            internal_for_leaf_vk_commit: value.internal_for_leaf_vk_commit.to_vk_commit(),
            internal_recursive_vk_commit: value.internal_recursive_vk_commit.to_vk_commit(),
            expected_def_hook_commit: value.expected_def_hook_commit.map(|commit| commit.to_digest()),
        }
    }
}

#[serde_as]
#[derive(Clone, Debug, Deserialize)]
pub struct VersionedVmStarkProof {
    pub version: String,
    #[serde_as(as = "serde_with::hex::Hex")]
    pub proof: Vec<u8>,
    #[serde_as(as = "serde_with::hex::Hex")]
    pub user_pvs_proof: Vec<u8>,
    #[serde_as(as = "Option<serde_with::hex::Hex>")]
    pub deferral_merkle_proofs: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct VmStarkProof {
    pub inner: Proof<SC>,
    pub user_pvs_proof: UserPublicValuesProof<DIGEST_SIZE, F>,
    pub deferral_merkle_proofs: Option<Vec<u8>>,
}

impl TryFrom<VersionedVmStarkProof> for VmStarkProof {
    type Error = std::io::Error;

    fn try_from(value: VersionedVmStarkProof) -> Result<Self, Self::Error> {
        let inner = Proof::<SC>::decode(&mut Cursor::new(&value.proof))?;
        let user_pvs_proof =
            UserPublicValuesProof::<DIGEST_SIZE, F>::decode::<SC, _>(&mut Cursor::new(
                &value.user_pvs_proof,
            ))?;

        Ok(Self {
            inner,
            user_pvs_proof,
            deferral_merkle_proofs: value.deferral_merkle_proofs,
        })
    }
}