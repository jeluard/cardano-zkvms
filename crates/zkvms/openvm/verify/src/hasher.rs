use std::convert::TryInto;

use openvm_poseidon2_air::{p3_symmetric::Permutation, Poseidon2Config};
use openvm_stark_backend::p3_field::{Field, PrimeCharacteristicRing};
use openvm_stark_sdk::p3_baby_bear::{BabyBear, Poseidon2BabyBear};

use crate::types::Digest;

pub trait Hasher<const CHUNK: usize, F: Field> {
    fn compress(&self, left: &[F; CHUNK], right: &[F; CHUNK]) -> [F; CHUNK];

    fn hash(&self, values: &[F; CHUNK]) -> [F; CHUNK] {
        self.compress(values, &[F::ZERO; CHUNK])
    }

    fn merkle_root(&self, values: &[F]) -> [F; CHUNK] {
        assert_eq!(values.len() % CHUNK, 0, "leaf width mismatch");
        assert!((values.len() / CHUNK).is_power_of_two(), "non-full Merkle tree");

        let mut nodes: Vec<_> = values
            .chunks_exact(CHUNK)
            .map(|chunk| self.hash(chunk.try_into().expect("chunked by CHUNK")))
            .collect();

        while nodes.len() > 1 {
            nodes = nodes
                .chunks_exact(2)
                .map(|pair| self.compress(&pair[0], &pair[1]))
                .collect();
        }

        nodes.pop().expect("at least one leaf")
    }
}

pub struct Poseidon2Hasher<F: Field> {
    permutation: Poseidon2BabyBear<16>,
    _marker: std::marker::PhantomData<F>,
}

pub fn vm_poseidon2_hasher() -> Poseidon2Hasher<BabyBear> {
    let config = Poseidon2Config::<BabyBear>::default();
    let (external_constants, internal_constants) =
        config.constants.to_external_internal_constants();

    Poseidon2Hasher {
        permutation: Poseidon2BabyBear::<16>::new(external_constants, internal_constants),
        _marker: std::marker::PhantomData,
    }
}

impl Hasher<8, BabyBear> for Poseidon2Hasher<BabyBear> {
    fn compress(&self, left: &Digest, right: &Digest) -> Digest {
        let mut input = [BabyBear::ZERO; 16];
        input[..8].copy_from_slice(left);
        input[8..].copy_from_slice(right);
        let output = self.permutation.permute(input);
        output[..8].try_into().expect("fixed digest width")
    }
}