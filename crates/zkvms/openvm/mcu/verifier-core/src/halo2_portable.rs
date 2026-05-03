#![allow(dead_code, unused_imports)]

use alloc::{boxed::Box, collections::BTreeMap, vec, vec::Vec};
use core::{cmp::Ordering, iter, ops::Neg};

use halo2curves::{
    bn256::{self, Fq, Fr, G1Affine, G2Affine, G1},
    ff::{Field, PrimeField},
    group::{Curve, Group},
    pairing::MillerLoopResult,
    serde::SerdeObject,
    CurveAffine,
};
use crate::{ProofEnvelope, VerifierKey, OPENVM_EVM_HALO2_PROOF_DATA_LEN};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
#[cfg(feature = "halo2-std")]
use snark_verifier_sdk::snark_verifier::{
    halo2_base::halo2_proofs::halo2curves::{
        bn256::{Fr as NativeFr, G1Affine as NativeG1Affine},
        ff::PrimeField as NativePrimeField,
        serde::SerdeObject as NativeSerdeObject,
    },
    verifier::plonk::PlonkProtocol as NativePlonkProtocol,
};

use crate::{
    decode_portable_halo2_key, is_portable_halo2_key_payload, EncodedG1, EncodedG2, VerifyError,
    BN254_SCALAR_BYTES, OPENVM_EVM_HALO2_ACCUMULATOR_WORDS, OPENVM_EVM_HALO2_PROOF_WORDS,
};

const LIMBS: usize = 3;
const FR_MODULUS: [u64; 4] = [
    0x43e1f593f0000001,
    0x2833e84879b97091,
    0xb85045b68181585d,
    0x30644e72e131a029,
];

#[cfg(feature = "halo2-std")]
#[derive(Clone, Debug, Deserialize)]
struct NativePortablePlonkProtocol {
    domain: NativePortableDomain,
    domain_as_witness: Option<NativePortableDomainAsWitness>,
    preprocessed: Vec<NativeG1Affine>,
    num_instance: Vec<usize>,
    num_witness: Vec<usize>,
    num_challenge: Vec<usize>,
    evaluations: Vec<NativePortableQuery>,
    queries: Vec<NativePortableQuery>,
    quotient: NativePortableQuotientPolynomial,
    transcript_initial_state: Option<NativeFr>,
    instance_committing_key: Option<NativePortableInstanceCommittingKey>,
    linearization: Option<NativePortableLinearizationStrategy>,
    accumulator_indices: Vec<Vec<(usize, usize)>>,
}

#[cfg(feature = "halo2-std")]
#[derive(Clone, Debug, Deserialize)]
struct NativePortableDomain {
    k: usize,
    n: usize,
    n_inv: NativeFr,
    gen: NativeFr,
    gen_inv: NativeFr,
}

#[cfg(feature = "halo2-std")]
#[derive(Clone, Debug, Deserialize)]
struct NativePortableDomainAsWitness {
    k: NativeFr,
    n: NativeFr,
    gen: NativeFr,
    gen_inv: NativeFr,
}

#[cfg(feature = "halo2-std")]
#[derive(Clone, Copy, Debug, Deserialize)]
struct NativePortableRotation(i32);

#[cfg(feature = "halo2-std")]
#[derive(Clone, Copy, Debug, Deserialize)]
struct NativePortableQuery {
    poly: usize,
    rotation: NativePortableRotation,
}

#[cfg(feature = "halo2-std")]
#[derive(Clone, Copy, Debug, Deserialize)]
enum NativePortableCommonPolynomial {
    Identity,
    Lagrange(i32),
}

#[cfg(feature = "halo2-std")]
#[derive(Clone, Debug, Deserialize)]
enum NativePortableExpression {
    Constant(NativeFr),
    CommonPolynomial(NativePortableCommonPolynomial),
    Polynomial(NativePortableQuery),
    Challenge(usize),
    Negated(Box<NativePortableExpression>),
    Sum(Box<NativePortableExpression>, Box<NativePortableExpression>),
    Product(Box<NativePortableExpression>, Box<NativePortableExpression>),
    Scaled(Box<NativePortableExpression>, NativeFr),
    DistributePowers(Vec<NativePortableExpression>, Box<NativePortableExpression>),
}

#[cfg(feature = "halo2-std")]
#[derive(Clone, Debug, Deserialize)]
struct NativePortableQuotientPolynomial {
    chunk_degree: usize,
    numerator: NativePortableExpression,
}

#[cfg(feature = "halo2-std")]
#[derive(Clone, Copy, Debug, Deserialize)]
enum NativePortableLinearizationStrategy {
    WithoutConstant,
    MinusVanishingTimesQuotient,
}

#[cfg(feature = "halo2-std")]
#[derive(Clone, Debug, Deserialize)]
struct NativePortableInstanceCommittingKey {
    bases: Vec<NativeG1Affine>,
    constant: Option<NativeG1Affine>,
}

#[cfg(feature = "halo2-std")]
pub(crate) fn convert_native_protocol(
    protocol: &NativePlonkProtocol<NativeG1Affine>,
) -> Result<PortablePlonkProtocol, VerifyError> {
    let shadow: NativePortablePlonkProtocol = bincode::serialize(protocol)
        .map_err(|_| VerifyError::InvalidHalo2VerifierKey)
        .and_then(|bytes| {
            bincode::deserialize(&bytes).map_err(|_| VerifyError::InvalidHalo2VerifierKey)
        })?;

    Ok(PortablePlonkProtocol {
        domain: convert_native_domain(&shadow.domain)?,
        domain_as_witness: shadow
            .domain_as_witness
            .as_ref()
            .map(convert_native_domain_as_witness)
            .transpose()?,
        preprocessed: shadow
            .preprocessed
            .iter()
            .map(convert_native_g1)
            .collect::<Result<Vec<_>, _>>()?,
        num_instance: shadow.num_instance,
        num_witness: shadow.num_witness,
        num_challenge: shadow.num_challenge,
        evaluations: shadow.evaluations.iter().map(convert_native_query).collect(),
        queries: shadow.queries.iter().map(convert_native_query).collect(),
        quotient: convert_native_quotient(&shadow.quotient)?,
        transcript_initial_state: shadow
            .transcript_initial_state
            .as_ref()
            .map(convert_native_scalar)
            .transpose()?,
        instance_committing_key: shadow
            .instance_committing_key
            .as_ref()
            .map(convert_native_instance_committing_key)
            .transpose()?,
        linearization: shadow.linearization.map(convert_native_linearization),
        accumulator_indices: shadow.accumulator_indices,
    })
}

#[cfg(feature = "halo2-std")]
fn convert_native_domain(domain: &NativePortableDomain) -> Result<PortableDomain, VerifyError> {
    Ok(PortableDomain {
        k: domain.k,
        n: domain.n,
        n_inv: convert_native_scalar(&domain.n_inv)?,
        gen: convert_native_scalar(&domain.gen)?,
        gen_inv: convert_native_scalar(&domain.gen_inv)?,
    })
}

#[cfg(feature = "halo2-std")]
fn convert_native_domain_as_witness(
    domain: &NativePortableDomainAsWitness,
) -> Result<PortableDomainAsWitness, VerifyError> {
    Ok(PortableDomainAsWitness {
        k: convert_native_scalar(&domain.k)?,
        n: convert_native_scalar(&domain.n)?,
        gen: convert_native_scalar(&domain.gen)?,
        gen_inv: convert_native_scalar(&domain.gen_inv)?,
    })
}

#[cfg(feature = "halo2-std")]
fn convert_native_g1(point: &NativeG1Affine) -> Result<G1Affine, VerifyError> {
    G1Affine::from_raw_bytes(point.to_raw_bytes().as_ref())
        .ok_or(VerifyError::InvalidHalo2VerifierKey)
}

#[cfg(feature = "halo2-std")]
fn convert_native_scalar(value: &NativeFr) -> Result<Fr, VerifyError> {
    let mut repr = [0u8; BN254_SCALAR_BYTES];
    repr.copy_from_slice(value.to_repr().as_ref());
    Fr::from_repr(repr.into())
        .into_option()
        .ok_or(VerifyError::InvalidHalo2VerifierKey)
}

#[cfg(feature = "halo2-std")]
fn convert_native_query(query: &NativePortableQuery) -> PlonkQuery {
    PlonkQuery {
        poly: query.poly,
        rotation: convert_native_rotation(query.rotation),
    }
}

#[cfg(feature = "halo2-std")]
fn convert_native_rotation(rotation: NativePortableRotation) -> Rotation {
    Rotation(rotation.0)
}

#[cfg(feature = "halo2-std")]
fn convert_native_quotient(
    quotient: &NativePortableQuotientPolynomial,
) -> Result<QuotientPolynomial, VerifyError> {
    Ok(QuotientPolynomial {
        chunk_degree: quotient.chunk_degree,
        numerator: convert_native_expression(&quotient.numerator)?,
    })
}

#[cfg(feature = "halo2-std")]
fn convert_native_expression(
    expression: &NativePortableExpression,
) -> Result<Expression, VerifyError> {
    Ok(match expression {
        NativePortableExpression::Constant(value) => {
            Expression::Constant(convert_native_scalar(value)?)
        }
        NativePortableExpression::CommonPolynomial(value) => {
            Expression::CommonPolynomial(convert_native_common_polynomial(*value))
        }
        NativePortableExpression::Polynomial(query) => {
            Expression::Polynomial(convert_native_query(query))
        }
        NativePortableExpression::Challenge(index) => Expression::Challenge(*index),
        NativePortableExpression::Negated(expr) => {
            Expression::Negated(Box::new(convert_native_expression(expr)?))
        }
        NativePortableExpression::Sum(lhs, rhs) => Expression::Sum(
            Box::new(convert_native_expression(lhs)?),
            Box::new(convert_native_expression(rhs)?),
        ),
        NativePortableExpression::Product(lhs, rhs) => Expression::Product(
            Box::new(convert_native_expression(lhs)?),
            Box::new(convert_native_expression(rhs)?),
        ),
        NativePortableExpression::Scaled(expr, scalar) => Expression::Scaled(
            Box::new(convert_native_expression(expr)?),
            convert_native_scalar(scalar)?,
        ),
        NativePortableExpression::DistributePowers(exprs, scalar) => {
            Expression::DistributePowers(
            exprs
                .iter()
                .map(convert_native_expression)
                .collect::<Result<Vec<_>, _>>()?,
            Box::new(convert_native_expression(scalar)?),
        )
        }
    })
}

#[cfg(feature = "halo2-std")]
fn convert_native_common_polynomial(value: NativePortableCommonPolynomial) -> CommonPolynomial {
    match value {
        NativePortableCommonPolynomial::Identity => CommonPolynomial::Identity,
        NativePortableCommonPolynomial::Lagrange(index) => CommonPolynomial::Lagrange(index),
    }
}

#[cfg(feature = "halo2-std")]
fn convert_native_instance_committing_key(
    key: &NativePortableInstanceCommittingKey,
) -> Result<InstanceCommittingKey, VerifyError> {
    Ok(InstanceCommittingKey {
        bases: key
            .bases
            .iter()
            .map(convert_native_g1)
            .collect::<Result<Vec<_>, _>>()?,
        constant: key.constant.as_ref().map(convert_native_g1).transpose()?,
    })
}

#[cfg(feature = "halo2-std")]
fn convert_native_linearization(
    value: NativePortableLinearizationStrategy,
) -> LinearizationStrategy {
    match value {
        NativePortableLinearizationStrategy::WithoutConstant => {
            LinearizationStrategy::WithoutConstant
        }
        NativePortableLinearizationStrategy::MinusVanishingTimesQuotient => {
            LinearizationStrategy::MinusVanishingTimesQuotient
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortableHalo2FrontendReport {
    pub instance_count: usize,
    pub preprocessed_commitments: usize,
    pub first_challenge_be: Option<[u8; BN254_SCALAR_BYTES]>,
}

pub fn verify_frontend(
    key: &VerifierKey,
    proof: &ProofEnvelope,
    on_step: fn(u8, &'static str),
) -> Result<PortableHalo2FrontendReport, VerifyError> {
    if !is_portable_halo2_key_payload(&key.payload) {
        return Err(VerifyError::CryptoBackendUnavailable);
    }

    let compact_key = decode_portable_halo2_key(&key.payload)?;
    if compact_key.proof_shape.accumulator_words != OPENVM_EVM_HALO2_ACCUMULATOR_WORDS
        || compact_key.proof_shape.proof_words != OPENVM_EVM_HALO2_PROOF_WORDS
        || proof.proof_data.len() != OPENVM_EVM_HALO2_PROOF_DATA_LEN
    {
        return Err(VerifyError::InvalidHalo2VerifierKey);
    }

    let protocol: PortablePlonkProtocol = postcard::from_bytes(&compact_key.protocol)
        .map_err(|_| VerifyError::InvalidHalo2VerifierKey)?;
    let g1 = decode_g1(&compact_key.g1)?;
    let g2 = decode_g2(&compact_key.g2)?;
    let s_g2 = decode_g2(&compact_key.s_g2)?;

    let instances = decode_openvm_instances(proof)?;
    let grouped_instances = group_instances(&protocol, instances)?;
    let instance_count = grouped_instances.iter().map(Vec::len).sum();

    let proof_without_accumulator =
        &proof.proof_data[OPENVM_EVM_HALO2_ACCUMULATOR_WORDS as usize * BN254_SCALAR_BYTES..];
    let mut transcript = EvmTranscript::new(proof_without_accumulator);
    let first_challenge_be = protocol.transcript_initial_state.map(fr_to_be);
    on_step(1, "transcript");
    let plonk_proof = PortablePlonkProof::read(&protocol, &grouped_instances, &mut transcript)?;
    verify_plonk(&protocol, &grouped_instances, &plonk_proof, g1, g2, s_g2, on_step)?;

    Ok(PortableHalo2FrontendReport {
        instance_count,
        preprocessed_commitments: protocol.preprocessed.len(),
        first_challenge_be,
    })
}

pub fn debug_verify_frontend(
    key: &VerifierKey,
    proof: &ProofEnvelope,
) -> (&'static str, Result<PortableHalo2FrontendReport, VerifyError>) {
    if !is_portable_halo2_key_payload(&key.payload) {
        return (
            "portable header",
            Err(VerifyError::CryptoBackendUnavailable),
        );
    }

    let compact_key = match decode_portable_halo2_key(&key.payload) {
        Ok(key) => key,
        Err(error) => return ("portable key decode", Err(error)),
    };

    if compact_key.proof_shape.accumulator_words != OPENVM_EVM_HALO2_ACCUMULATOR_WORDS
        || compact_key.proof_shape.proof_words != OPENVM_EVM_HALO2_PROOF_WORDS
        || proof.proof_data.len() != OPENVM_EVM_HALO2_PROOF_DATA_LEN
    {
        return ("proof shape", Err(VerifyError::InvalidHalo2VerifierKey));
    }

    let protocol: PortablePlonkProtocol = match postcard::from_bytes(&compact_key.protocol) {
        Ok(protocol) => protocol,
        Err(_) => return ("protocol decode", Err(VerifyError::InvalidHalo2VerifierKey)),
    };
    let g1 = match decode_g1(&compact_key.g1) {
        Ok(g1) => g1,
        Err(error) => return ("g1 decode", Err(error)),
    };
    let g2 = match decode_g2(&compact_key.g2) {
        Ok(g2) => g2,
        Err(error) => return ("g2 decode", Err(error)),
    };
    let s_g2 = match decode_g2(&compact_key.s_g2) {
        Ok(s_g2) => s_g2,
        Err(error) => return ("s_g2 decode", Err(error)),
    };

    let instances = match decode_openvm_instances(proof) {
        Ok(instances) => instances,
        Err(error) => return ("instance decode", Err(error)),
    };
    let grouped_instances = match group_instances(&protocol, instances) {
        Ok(instances) => instances,
        Err(error) => return ("instance grouping", Err(error)),
    };
    let instance_count = grouped_instances.iter().map(Vec::len).sum();

    let proof_without_accumulator =
        &proof.proof_data[OPENVM_EVM_HALO2_ACCUMULATOR_WORDS as usize * BN254_SCALAR_BYTES..];
    let mut transcript = EvmTranscript::new(proof_without_accumulator);
    let first_challenge_be = protocol.transcript_initial_state.map(fr_to_be);
    let plonk_proof = match PortablePlonkProof::read(&protocol, &grouped_instances, &mut transcript)
    {
        Ok(proof) => proof,
        Err(error) => return ("proof decode", Err(error)),
    };
    fn noop(_: u8, _: &'static str) {}
    if let Err(error) = verify_plonk(&protocol, &grouped_instances, &plonk_proof, g1, g2, s_g2, noop) {
        return ("verify plonk", Err(error));
    }

    (
        "verified",
        Ok(PortableHalo2FrontendReport {
            instance_count,
            preprocessed_commitments: protocol.preprocessed.len(),
            first_challenge_be,
        }),
    )
}

fn group_instances(
    protocol: &PortablePlonkProtocol,
    instances: Vec<Fr>,
) -> Result<Vec<Vec<Fr>>, VerifyError> {
    let expected = protocol
        .num_instance
        .iter()
        .try_fold(0usize, |acc, value| acc.checked_add(*value))
        .ok_or(VerifyError::InvalidHalo2VerifierKey)?;
    if expected != instances.len() {
        return Err(VerifyError::InvalidHalo2VerifierKey);
    }

    let mut offset = 0;
    Ok(protocol
        .num_instance
        .iter()
        .map(|len| {
            let group = instances[offset..offset + *len].to_vec();
            offset += *len;
            group
        })
        .collect())
}

fn verify_plonk(
    protocol: &PortablePlonkProtocol,
    instances: &[Vec<Fr>],
    proof: &PortablePlonkProof,
    svk_g: G1Affine,
    g2: G2Affine,
    s_g2: G2Affine,
    on_step: fn(u8, &'static str),
) -> Result<(), VerifyError> {
    on_step(2, "lagrange");
    let common = CommonPolynomialEvaluation::new(&protocol.domain, protocol.langranges(), proof.z)?;
    on_step(3, "constraints");
    let mut evaluations = proof.evaluations(protocol, instances, &common)?;
    on_step(4, "commitments");
    let commitments = proof.commitments(protocol, &common, &mut evaluations)?;
    on_step(5, "queries");
    let queries = proof.queries(protocol, evaluations)?;
    on_step(6, "shplonk");
    let accumulator = verify_shplonk(&commitments, proof.z, &queries, &proof.pcs, svk_g)?;

    on_step(7, "pairing");
    decide_kzg(accumulator, g2, s_g2)?;
    for old_accumulator in &proof.old_accumulators {
        on_step(7, "pairing");
        decide_kzg(old_accumulator.clone(), g2, s_g2)?;
    }
    Ok(())
}

fn verify_shplonk(
    commitments: &[Msm],
    z: Fr,
    queries: &[PcsQuery],
    proof: &Bdfg21Proof,
    svk_g: G1Affine,
) -> Result<KzgAccumulator, VerifyError> {
    let sets = query_sets(queries);
    if sets.is_empty() {
        return Err(VerifyError::Halo2VerificationFailed);
    }
    let coeffs = query_set_coeffs(&sets, z, proof.z_prime)?;
    let powers_of_mu = powers(
        proof.mu,
        sets.iter().map(|set| set.polys.len()).max().unwrap(),
    );
    let powers_of_gamma = powers(proof.gamma, sets.len());

    let mut f = Msm::default();
    for ((set, coeff), power_of_gamma) in sets.iter().zip(&coeffs).zip(powers_of_gamma) {
        let msm = set.msm(coeff, commitments, &powers_of_mu)?;
        f = f + msm * power_of_gamma;
    }
    f = f - Msm::base(proof.w) * coeffs[0].z_s;

    let rhs = Msm::base(proof.w_prime);
    let lhs = f + rhs.clone() * proof.z_prime;
    Ok(KzgAccumulator {
        lhs: lhs.evaluate(svk_g),
        rhs: rhs.evaluate(svk_g),
    })
}

fn decide_kzg(
    accumulator: KzgAccumulator,
    g2: G2Affine,
    s_g2: G2Affine,
) -> Result<(), VerifyError> {
    let neg_s_g2 = s_g2.neg();
    let result =
        bn256::multi_miller_loop(&[(&accumulator.lhs, &g2), (&accumulator.rhs, &neg_s_g2)])
            .final_exponentiation();
    bool::from(result.is_identity())
        .then_some(())
        .ok_or(VerifyError::Halo2VerificationFailed)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PortablePlonkProtocol {
    domain: PortableDomain,
    domain_as_witness: Option<PortableDomainAsWitness>,
    preprocessed: Vec<G1Affine>,
    num_instance: Vec<usize>,
    num_witness: Vec<usize>,
    num_challenge: Vec<usize>,
    evaluations: Vec<PlonkQuery>,
    queries: Vec<PlonkQuery>,
    quotient: QuotientPolynomial,
    transcript_initial_state: Option<Fr>,
    instance_committing_key: Option<InstanceCommittingKey>,
    linearization: Option<LinearizationStrategy>,
    accumulator_indices: Vec<Vec<(usize, usize)>>,
}

impl PortablePlonkProtocol {
    fn langranges(&self) -> Vec<i32> {
        let mut values = self.quotient.numerator.used_lagrange();
        if self.instance_committing_key.is_none() {
            let offset = self.preprocessed.len();
            let range = offset..offset + self.num_instance.len();
            let mut min_rotation = 0;
            let mut max_rotation = 0;
            for query in self
                .quotient
                .numerator
                .used_query()
                .into_iter()
                .filter(|query| range.contains(&query.poly))
            {
                if query.rotation.0 < min_rotation {
                    min_rotation = query.rotation.0;
                } else if query.rotation.0 > max_rotation {
                    max_rotation = query.rotation.0;
                }
            }
            let max_instance_len = self.num_instance.iter().max().copied().unwrap_or_default();
            values.extend(-max_rotation..max_instance_len as i32 + min_rotation.abs());
        }
        values.sort_unstable();
        values.dedup();
        values
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
struct PortableDomain {
    k: usize,
    n: usize,
    n_inv: Fr,
    gen: Fr,
    gen_inv: Fr,
}

impl PortableDomain {
    fn rotate_scalar(&self, scalar: Fr, rotation: Rotation) -> Fr {
        match rotation.0.cmp(&0) {
            Ordering::Equal => scalar,
            Ordering::Greater => scalar * self.gen.pow_vartime([rotation.0 as u64]),
            Ordering::Less => scalar * self.gen_inv.pow_vartime([(-rotation.0) as u64]),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
struct PortableDomainAsWitness {
    k: Fr,
    n: Fr,
    gen: Fr,
    gen_inv: Fr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
struct Rotation(i32);

impl Rotation {
    const fn cur() -> Self {
        Self(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
struct PlonkQuery {
    poly: usize,
    rotation: Rotation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct QuotientPolynomial {
    chunk_degree: usize,
    numerator: Expression,
}

impl QuotientPolynomial {
    fn num_chunk(&self) -> usize {
        let degree = self.numerator.degree().saturating_sub(1);
        degree.div_ceil(self.chunk_degree)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
enum CommonPolynomial {
    Identity,
    Lagrange(i32),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
enum Expression {
    Constant(Fr),
    CommonPolynomial(CommonPolynomial),
    Polynomial(PlonkQuery),
    Challenge(usize),
    Negated(Box<Expression>),
    Sum(Box<Expression>, Box<Expression>),
    Product(Box<Expression>, Box<Expression>),
    Scaled(Box<Expression>, Fr),
    DistributePowers(Vec<Expression>, Box<Expression>),
}

impl Expression {
    fn degree(&self) -> usize {
        match self {
            Self::Constant(_) | Self::Challenge(_) => 0,
            Self::CommonPolynomial(_) | Self::Polynomial(_) => 1,
            Self::Negated(expr) | Self::Scaled(expr, _) => expr.degree(),
            Self::Sum(lhs, rhs) => lhs.degree().max(rhs.degree()),
            Self::Product(lhs, rhs) => lhs.degree() + rhs.degree(),
            Self::DistributePowers(exprs, scalar) => exprs
                .iter()
                .chain(iter::once(scalar.as_ref()))
                .map(Self::degree)
                .max()
                .unwrap_or_default(),
        }
    }

    fn used_lagrange(&self) -> Vec<i32> {
        let mut values = Vec::new();
        self.collect_lagrange(&mut values);
        values.sort_unstable();
        values.dedup();
        values
    }

    fn collect_lagrange(&self, values: &mut Vec<i32>) {
        match self {
            Self::CommonPolynomial(CommonPolynomial::Lagrange(index)) => values.push(*index),
            Self::Negated(expr) | Self::Scaled(expr, _) => expr.collect_lagrange(values),
            Self::Sum(lhs, rhs) | Self::Product(lhs, rhs) => {
                lhs.collect_lagrange(values);
                rhs.collect_lagrange(values);
            }
            Self::DistributePowers(exprs, scalar) => {
                for expr in exprs {
                    expr.collect_lagrange(values);
                }
                scalar.collect_lagrange(values);
            }
            _ => {}
        }
    }

    fn used_query(&self) -> Vec<PlonkQuery> {
        let mut values = Vec::new();
        self.collect_query(&mut values);
        values.sort_unstable();
        values.dedup();
        values
    }

    fn collect_query(&self, values: &mut Vec<PlonkQuery>) {
        match self {
            Self::Polynomial(query) => values.push(*query),
            Self::Negated(expr) | Self::Scaled(expr, _) => expr.collect_query(values),
            Self::Sum(lhs, rhs) | Self::Product(lhs, rhs) => {
                lhs.collect_query(values);
                rhs.collect_query(values);
            }
            Self::DistributePowers(exprs, scalar) => {
                for expr in exprs {
                    expr.collect_query(values);
                }
                scalar.collect_query(values);
            }
            _ => {}
        }
    }

    fn evaluate_msm(
        &self,
        common: &CommonPolynomialEvaluation,
        evaluations: &BTreeMap<PlonkQuery, Fr>,
        commitments: &[Msm],
        challenges: &[Fr],
    ) -> Result<Msm, VerifyError> {
        match self {
            Self::Constant(scalar) => Ok(Msm::constant(*scalar)),
            Self::CommonPolynomial(poly) => Ok(Msm::constant(common.get(*poly)?)),
            Self::Polynomial(query) => evaluations
                .get(query)
                .copied()
                .map(Msm::constant)
                .or_else(|| {
                    (query.rotation == Rotation::cur())
                        .then(|| commitments.get(query.poly).cloned())
                        .flatten()
                })
                .ok_or(VerifyError::Halo2VerificationFailed),
            Self::Challenge(index) => challenges
                .get(*index)
                .copied()
                .map(Msm::constant)
                .ok_or(VerifyError::Halo2VerificationFailed),
            Self::Negated(expr) => {
                Ok(-expr.evaluate_msm(common, evaluations, commitments, challenges)?)
            }
            Self::Sum(lhs, rhs) => {
                Ok(
                    lhs.evaluate_msm(common, evaluations, commitments, challenges)?
                        + rhs.evaluate_msm(common, evaluations, commitments, challenges)?,
                )
            }
            Self::Product(lhs, rhs) => {
                let lhs = lhs.evaluate_msm(common, evaluations, commitments, challenges)?;
                let rhs = rhs.evaluate_msm(common, evaluations, commitments, challenges)?;
                match (lhs.size(), rhs.size()) {
                    (0, _) => Ok(rhs * lhs.try_into_constant()?),
                    (_, 0) => Ok(lhs * rhs.try_into_constant()?),
                    _ => Err(VerifyError::Halo2VerificationFailed),
                }
            }
            Self::Scaled(expr, scalar) => {
                Ok(expr.evaluate_msm(common, evaluations, commitments, challenges)? * *scalar)
            }
            Self::DistributePowers(exprs, scalar) => {
                if exprs.is_empty() {
                    return Err(VerifyError::Halo2VerificationFailed);
                }
                let scalar = scalar
                    .evaluate_msm(common, evaluations, commitments, challenges)?
                    .try_into_constant()?;
                let mut iter = exprs.iter();
                let first = iter
                    .next()
                    .ok_or(VerifyError::Halo2VerificationFailed)?
                    .evaluate_msm(common, evaluations, commitments, challenges)?;
                iter.try_fold(first, |acc, expr| {
                    Ok(acc * scalar
                        + expr.evaluate_msm(common, evaluations, commitments, challenges)?)
                })
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
enum LinearizationStrategy {
    WithoutConstant,
    MinusVanishingTimesQuotient,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct InstanceCommittingKey {
    bases: Vec<G1Affine>,
    constant: Option<G1Affine>,
}

#[derive(Clone, Debug)]
struct PortablePlonkProof {
    committed_instances: Option<Vec<G1Affine>>,
    witnesses: Vec<G1Affine>,
    challenges: Vec<Fr>,
    quotients: Vec<G1Affine>,
    z: Fr,
    evaluations: Vec<Fr>,
    pcs: Bdfg21Proof,
    old_accumulators: Vec<KzgAccumulator>,
}

impl PortablePlonkProof {
    fn read(
        protocol: &PortablePlonkProtocol,
        instances: &[Vec<Fr>],
        transcript: &mut EvmTranscript<'_>,
    ) -> Result<Self, VerifyError> {
        if let Some(transcript_initial_state) = protocol.transcript_initial_state {
            transcript.common_scalar(transcript_initial_state);
        }
        if protocol.num_instance != instances.iter().map(Vec::len).collect::<Vec<_>>() {
            return Err(VerifyError::InvalidHalo2VerifierKey);
        }

        let committed_instances = if let Some(ick) = &protocol.instance_committing_key {
            let committed = instances
                .iter()
                .map(|instance_values| {
                    let mut msm = Msm::default();
                    for (scalar, base) in instance_values.iter().zip(&ick.bases) {
                        msm = msm + Msm::base(*base) * *scalar;
                    }
                    if let Some(constant) = ick.constant {
                        msm = msm + Msm::base(constant);
                    }
                    msm.evaluate(G1Affine::generator())
                })
                .collect::<Vec<_>>();
            for point in &committed {
                transcript.common_ec_point(*point);
            }
            Some(committed)
        } else {
            for values in instances {
                for value in values {
                    transcript.common_scalar(*value);
                }
            }
            None
        };

        let mut witnesses = Vec::new();
        let mut challenges = Vec::new();
        for (num_witness, num_challenge) in protocol.num_witness.iter().zip(&protocol.num_challenge)
        {
            for _ in 0..*num_witness {
                witnesses.push(transcript.read_ec_point()?);
            }
            for _ in 0..*num_challenge {
                challenges.push(transcript.squeeze_challenge());
            }
        }

        let mut quotients = Vec::new();
        for _ in 0..protocol.quotient.num_chunk() {
            quotients.push(transcript.read_ec_point()?);
        }
        let z = transcript.squeeze_challenge();
        let mut evaluations = Vec::new();
        for _ in 0..protocol.evaluations.len() {
            evaluations.push(transcript.read_scalar()?);
        }
        let pcs = Bdfg21Proof::read(transcript)?;
        let old_accumulators = protocol
            .accumulator_indices
            .iter()
            .map(|indices| accumulator_from_repr(indices, instances))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            committed_instances,
            witnesses,
            challenges,
            quotients,
            z,
            evaluations,
            pcs,
            old_accumulators,
        })
    }

    fn evaluations(
        &self,
        protocol: &PortablePlonkProtocol,
        instances: &[Vec<Fr>],
        common: &CommonPolynomialEvaluation,
    ) -> Result<BTreeMap<PlonkQuery, Fr>, VerifyError> {
        let mut values = BTreeMap::new();
        if protocol.instance_committing_key.is_none() {
            let offset = protocol.preprocessed.len();
            let range = offset..offset + protocol.num_instance.len();
            for query in protocol
                .quotient
                .numerator
                .used_query()
                .into_iter()
                .filter(|query| range.contains(&query.poly))
            {
                let instance_values = &instances[query.poly - offset];
                let mut eval = Fr::ZERO;
                for (instance, i_minus_r) in instance_values.iter().zip(-query.rotation.0..) {
                    eval += *instance * common.get(CommonPolynomial::Lagrange(i_minus_r))?;
                }
                values.insert(query, eval);
            }
        }
        for (query, evaluation) in protocol.evaluations.iter().zip(&self.evaluations) {
            values.insert(*query, *evaluation);
        }
        Ok(values)
    }

    fn commitments(
        &self,
        protocol: &PortablePlonkProtocol,
        common: &CommonPolynomialEvaluation,
        evaluations: &mut BTreeMap<PlonkQuery, Fr>,
    ) -> Result<Vec<Msm>, VerifyError> {
        let mut commitments = Vec::new();
        commitments.extend(protocol.preprocessed.iter().copied().map(Msm::base));
        if let Some(committed_instances) = &self.committed_instances {
            commitments.extend(committed_instances.iter().copied().map(Msm::base));
        } else {
            commitments.extend(iter::repeat_with(Msm::default).take(protocol.num_instance.len()));
        }
        commitments.extend(self.witnesses.iter().copied().map(Msm::base));

        let numerator = protocol.quotient.numerator.evaluate_msm(
            common,
            evaluations,
            &commitments,
            &self.challenges,
        )?;

        let quotient_query = PlonkQuery {
            poly: protocol.preprocessed.len() + protocol.num_instance.len() + self.witnesses.len(),
            rotation: Rotation::cur(),
        };
        let quotient = powers(
            common
                .zn
                .pow_vartime([protocol.quotient.chunk_degree as u64]),
            self.quotients.len(),
        )
        .into_iter()
        .zip(self.quotients.iter().copied().map(Msm::base))
        .fold(Msm::default(), |acc, (coeff, chunk)| acc + chunk * coeff);

        match protocol.linearization {
            Some(LinearizationStrategy::WithoutConstant) => {
                let linearization_query = PlonkQuery {
                    poly: quotient_query.poly + 1,
                    rotation: Rotation::cur(),
                };
                let (msm, constant) = numerator.split();
                commitments.push(quotient);
                commitments.push(msm);
                let linearization_eval = evaluations
                    .get(&linearization_query)
                    .copied()
                    .ok_or(VerifyError::Halo2VerificationFailed)?;
                evaluations.insert(
                    quotient_query,
                    (constant.unwrap_or(Fr::ZERO) + linearization_eval) * common.zn_minus_one_inv,
                );
            }
            Some(LinearizationStrategy::MinusVanishingTimesQuotient) => {
                let (msm, constant) = (numerator - quotient * common.zn_minus_one).split();
                commitments.push(msm);
                evaluations.insert(quotient_query, constant.unwrap_or(Fr::ZERO));
            }
            None => {
                commitments.push(quotient);
                evaluations.insert(
                    quotient_query,
                    numerator.try_into_constant()? * common.zn_minus_one_inv,
                );
            }
        }

        Ok(commitments)
    }

    fn queries(
        &self,
        protocol: &PortablePlonkProtocol,
        mut evaluations: BTreeMap<PlonkQuery, Fr>,
    ) -> Result<Vec<PcsQuery>, VerifyError> {
        let mut queries = Vec::new();
        for query in &protocol.queries {
            let loaded_shift = protocol.domain.rotate_scalar(Fr::ONE, query.rotation);
            let eval = evaluations
                .remove(query)
                .ok_or(VerifyError::Halo2VerificationFailed)?;
            queries.push(PcsQuery {
                poly: query.poly,
                shift: query.rotation,
                loaded_shift,
                eval,
            });
        }
        Ok(queries)
    }
}

#[derive(Clone, Copy, Debug)]
struct Bdfg21Proof {
    mu: Fr,
    gamma: Fr,
    w: G1Affine,
    z_prime: Fr,
    w_prime: G1Affine,
}

impl Bdfg21Proof {
    fn read(transcript: &mut EvmTranscript<'_>) -> Result<Self, VerifyError> {
        let mu = transcript.squeeze_challenge();
        let gamma = transcript.squeeze_challenge();
        let w = transcript.read_ec_point()?;
        let z_prime = transcript.squeeze_challenge();
        let w_prime = transcript.read_ec_point()?;
        Ok(Self {
            mu,
            gamma,
            w,
            z_prime,
            w_prime,
        })
    }
}

#[derive(Clone, Debug)]
struct KzgAccumulator {
    lhs: G1Affine,
    rhs: G1Affine,
}

#[derive(Clone, Copy, Debug)]
struct PcsQuery {
    poly: usize,
    shift: Rotation,
    loaded_shift: Fr,
    eval: Fr,
}

#[derive(Clone, Debug, Default)]
struct Msm {
    terms: Vec<(G1Affine, Fr)>,
    constant: Fr,
}

impl Msm {
    fn base(point: G1Affine) -> Self {
        Self {
            terms: vec![(point, Fr::ONE)],
            constant: Fr::ZERO,
        }
    }

    fn constant(constant: Fr) -> Self {
        Self {
            terms: Vec::new(),
            constant,
        }
    }

    fn size(&self) -> usize {
        self.terms.len()
    }

    fn try_into_constant(self) -> Result<Fr, VerifyError> {
        if self.terms.is_empty() {
            Ok(self.constant)
        } else {
            Err(VerifyError::Halo2VerificationFailed)
        }
    }

    fn split(self) -> (Self, Option<Fr>) {
        let constant = (self.constant != Fr::ZERO).then_some(self.constant);
        (
            Self {
                terms: self.terms,
                constant: Fr::ZERO,
            },
            constant,
        )
    }

    fn evaluate(&self, generator: G1Affine) -> G1Affine {
        let mut acc = G1::identity();
        for (point, scalar) in &self.terms {
            acc += point * scalar;
        }
        if self.constant != Fr::ZERO {
            acc += generator * self.constant;
        }
        acc.to_affine()
    }
}

impl core::ops::Add for Msm {
    type Output = Self;

    fn add(mut self, rhs: Self) -> Self::Output {
        self.terms.extend(rhs.terms);
        self.constant += rhs.constant;
        self
    }
}

impl core::ops::Sub for Msm {
    type Output = Self;

    fn sub(mut self, rhs: Self) -> Self::Output {
        self.terms.extend(
            rhs.terms
                .into_iter()
                .map(|(point, scalar)| (point, -scalar)),
        );
        self.constant -= rhs.constant;
        self
    }
}

impl core::ops::Neg for Msm {
    type Output = Self;

    fn neg(mut self) -> Self::Output {
        for (_, scalar) in &mut self.terms {
            *scalar = -*scalar;
        }
        self.constant = -self.constant;
        self
    }
}

impl core::ops::Mul<Fr> for Msm {
    type Output = Self;

    fn mul(mut self, rhs: Fr) -> Self::Output {
        for (_, scalar) in &mut self.terms {
            *scalar *= rhs;
        }
        self.constant *= rhs;
        self
    }
}

#[derive(Clone, Debug)]
struct CommonPolynomialEvaluation {
    zn: Fr,
    zn_minus_one: Fr,
    zn_minus_one_inv: Fr,
    identity: Fr,
    lagrange: BTreeMap<i32, Fr>,
}

impl CommonPolynomialEvaluation {
    fn new(domain: &PortableDomain, lagranges: Vec<i32>, z: Fr) -> Result<Self, VerifyError> {
        let zn = z.pow_vartime([domain.n as u64]);
        let zn_minus_one = zn - Fr::ONE;
        let zn_minus_one_inv = invert(zn_minus_one)?;
        let numer = zn_minus_one * domain.n_inv;
        let mut lagrange = BTreeMap::new();
        for index in lagranges {
            let omega = domain.rotate_scalar(Fr::ONE, Rotation(index));
            lagrange.insert(index, numer * omega * invert(z - omega)?);
        }
        Ok(Self {
            zn,
            zn_minus_one,
            zn_minus_one_inv,
            identity: z,
            lagrange,
        })
    }

    fn get(&self, poly: CommonPolynomial) -> Result<Fr, VerifyError> {
        match poly {
            CommonPolynomial::Identity => Ok(self.identity),
            CommonPolynomial::Lagrange(index) => self
                .lagrange
                .get(&index)
                .copied()
                .ok_or(VerifyError::Halo2VerificationFailed),
        }
    }
}

#[derive(Clone, Debug)]
struct QuerySet {
    shifts: Vec<(Rotation, Fr)>,
    polys: Vec<usize>,
    evals: Vec<Vec<Fr>>,
}

impl QuerySet {
    fn msm(
        &self,
        coeff: &QuerySetCoeff,
        commitments: &[Msm],
        powers_of_mu: &[Fr],
    ) -> Result<Msm, VerifyError> {
        self.polys
            .iter()
            .zip(&self.evals)
            .zip(powers_of_mu)
            .try_fold(Msm::default(), |acc, ((poly, evals), power_of_mu)| {
                let commitment = coeff
                    .commitment_coeff
                    .map(|commitment_coeff| commitments[*poly].clone() * commitment_coeff)
                    .unwrap_or_else(|| commitments[*poly].clone());
                let r_eval = coeff
                    .eval_coeffs
                    .iter()
                    .zip(evals)
                    .fold(Fr::ZERO, |sum, (coeff, eval)| sum + *coeff * eval)
                    * coeff.r_eval_coeff;
                Ok(acc + (commitment - Msm::constant(r_eval)) * *power_of_mu)
            })
    }
}

#[derive(Clone, Debug)]
struct QuerySetCoeff {
    z_s: Fr,
    eval_coeffs: Vec<Fr>,
    commitment_coeff: Option<Fr>,
    r_eval_coeff: Fr,
}

fn query_sets(queries: &[PcsQuery]) -> Vec<QuerySet> {
    let mut poly_shifts: Vec<(usize, Vec<(Rotation, Fr)>, Vec<Fr>)> = Vec::new();
    for query in queries {
        if let Some((_, shifts, evals)) = poly_shifts
            .iter_mut()
            .find(|(poly, _, _)| *poly == query.poly)
        {
            if !shifts.iter().any(|(shift, _)| *shift == query.shift) {
                shifts.push((query.shift, query.loaded_shift));
                evals.push(query.eval);
            }
        } else {
            poly_shifts.push((
                query.poly,
                vec![(query.shift, query.loaded_shift)],
                vec![query.eval],
            ));
        }
    }

    let mut sets: Vec<QuerySet> = Vec::new();
    for (poly, shifts, evals) in poly_shifts {
        if let Some(set) = sets
            .iter_mut()
            .find(|set| same_shifts(&set.shifts, &shifts))
        {
            if !set.polys.contains(&poly) {
                set.polys.push(poly);
                let aligned = set
                    .shifts
                    .iter()
                    .map(|lhs| {
                        let idx = shifts.iter().position(|rhs| lhs.0 == rhs.0).unwrap();
                        evals[idx]
                    })
                    .collect();
                set.evals.push(aligned);
            }
        } else {
            sets.push(QuerySet {
                shifts,
                polys: vec![poly],
                evals: vec![evals],
            });
        }
    }
    sets
}

fn same_shifts(lhs: &[(Rotation, Fr)], rhs: &[(Rotation, Fr)]) -> bool {
    lhs.len() == rhs.len()
        && lhs
            .iter()
            .all(|(shift, _)| rhs.iter().any(|(rhs_shift, _)| rhs_shift == shift))
}

fn query_set_coeffs(
    sets: &[QuerySet],
    z: Fr,
    z_prime: Fr,
) -> Result<Vec<QuerySetCoeff>, VerifyError> {
    let mut superset = BTreeMap::new();
    for set in sets {
        for (shift, loaded_shift) in &set.shifts {
            superset.insert(*shift, *loaded_shift);
        }
    }
    let size = sets
        .iter()
        .map(|set| set.shifts.len())
        .chain(Some(2))
        .max()
        .unwrap();
    let powers_of_z = powers(z, size);
    let z_prime_minus_z_shift_i = superset
        .into_iter()
        .map(|(shift, loaded_shift)| (shift, z_prime - z * loaded_shift))
        .collect::<BTreeMap<_, _>>();

    let mut z_s_1 = None;
    let mut coeffs = Vec::new();
    for set in sets {
        let coeff = QuerySetCoeff::new(
            &set.shifts,
            &powers_of_z,
            z_prime,
            &z_prime_minus_z_shift_i,
            z_s_1,
        )?;
        if z_s_1.is_none() {
            z_s_1 = Some(coeff.z_s);
        }
        coeffs.push(coeff);
    }
    Ok(coeffs)
}

impl QuerySetCoeff {
    fn new(
        shifts: &[(Rotation, Fr)],
        powers_of_z: &[Fr],
        z_prime: Fr,
        z_prime_minus_z_shift_i: &BTreeMap<Rotation, Fr>,
        z_s_1: Option<Fr>,
    ) -> Result<Self, VerifyError> {
        let mut normalized_ell_primes = Vec::new();
        for (j, shift_j) in shifts.iter().enumerate() {
            let mut value = Fr::ONE;
            for (i, shift_i) in shifts.iter().enumerate() {
                if i != j {
                    value *= shift_j.1 - shift_i.1;
                }
            }
            normalized_ell_primes.push(value);
        }

        let z_pow_k_minus_one = powers_of_z[shifts.len() - 1];
        let mut eval_coeffs = Vec::new();
        for ((_, loaded_shift), normalized_ell_prime) in shifts.iter().zip(&normalized_ell_primes) {
            let tmp = *normalized_ell_prime * z_pow_k_minus_one;
            let denom = tmp * z_prime - tmp * *loaded_shift * powers_of_z[1];
            eval_coeffs.push(invert(denom)?);
        }

        let z_s = shifts.iter().try_fold(Fr::ONE, |acc, (shift, _)| {
            Ok(acc
                * z_prime_minus_z_shift_i
                    .get(shift)
                    .copied()
                    .ok_or(VerifyError::Halo2VerificationFailed)?)
        })?;
        let commitment_coeff = z_s_1
            .map(|previous| invert(z_s).map(|inv| previous * inv))
            .transpose()?;
        let barycentric_weights_sum = eval_coeffs.iter().fold(Fr::ZERO, |acc, value| acc + value);
        let r_eval_coeff = commitment_coeff
            .map(|coeff| invert(barycentric_weights_sum).map(|inv| coeff * inv))
            .unwrap_or_else(|| invert(barycentric_weights_sum))?;

        Ok(Self {
            z_s,
            eval_coeffs,
            commitment_coeff,
            r_eval_coeff,
        })
    }
}

struct EvmTranscript<'a> {
    stream: &'a [u8],
    offset: usize,
    buffer: Vec<u8>,
}

impl<'a> EvmTranscript<'a> {
    fn new(stream: &'a [u8]) -> Self {
        Self {
            stream,
            offset: 0,
            buffer: Vec::new(),
        }
    }

    fn common_scalar(&mut self, scalar: Fr) {
        self.buffer.extend(fr_to_be(scalar));
    }

    fn common_ec_point(&mut self, point: G1Affine) {
        if let Some(coordinates) =
            Option::<halo2curves::Coordinates<G1Affine>>::from(point.coordinates())
        {
            let mut x = coordinates.x().to_repr();
            let mut y = coordinates.y().to_repr();
            x.as_mut().reverse();
            y.as_mut().reverse();
            self.buffer.extend_from_slice(x.as_ref());
            self.buffer.extend_from_slice(y.as_ref());
        }
    }

    fn squeeze_challenge(&mut self) -> Fr {
        let mut input = self.buffer.clone();
        if input.len() == BN254_SCALAR_BYTES {
            input.push(1);
        }
        let digest: [u8; 32] = Keccak256::digest(input).into();
        self.buffer.clear();
        self.buffer.extend_from_slice(&digest);
        fr_from_be_mod(digest)
    }

    fn read_scalar(&mut self) -> Result<Fr, VerifyError> {
        let bytes = self.read_array::<32>()?;
        let scalar = fr_from_be_checked(bytes)?;
        self.common_scalar(scalar);
        Ok(scalar)
    }

    fn read_ec_point(&mut self) -> Result<G1Affine, VerifyError> {
        let x = fq_from_be_checked(self.read_array::<32>()?)?;
        let y = fq_from_be_checked(self.read_array::<32>()?)?;
        let point = Option::<G1Affine>::from(G1Affine::from_xy(x, y))
            .ok_or(VerifyError::InvalidHalo2VerifierKey)?;
        self.common_ec_point(point);
        Ok(point)
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], VerifyError> {
        let end = self
            .offset
            .checked_add(N)
            .ok_or(VerifyError::Halo2VerificationFailed)?;
        let bytes = self
            .stream
            .get(self.offset..end)
            .ok_or(VerifyError::Halo2VerificationFailed)?;
        self.offset = end;
        let mut out = [0; N];
        out.copy_from_slice(bytes);
        Ok(out)
    }
}

fn decode_openvm_instances(proof: &ProofEnvelope) -> Result<Vec<Fr>, VerifyError> {
    let mut instances = Vec::with_capacity(
        OPENVM_EVM_HALO2_ACCUMULATOR_WORDS as usize + 2 + proof.user_public_values.len(),
    );

    for chunk in proof.proof_data
        [..OPENVM_EVM_HALO2_ACCUMULATOR_WORDS as usize * BN254_SCALAR_BYTES]
        .chunks_exact(BN254_SCALAR_BYTES)
    {
        let mut word = [0; BN254_SCALAR_BYTES];
        word.copy_from_slice(chunk);
        instances.push(fr_from_be_checked(word)?);
    }

    instances.push(fr_from_be_checked(proof.app_exe_commit)?);
    instances.push(fr_from_be_checked(proof.app_vm_commit)?);

    for byte in &proof.user_public_values {
        let mut word = [0; BN254_SCALAR_BYTES];
        word[BN254_SCALAR_BYTES - 1] = *byte;
        instances.push(fr_from_be_checked(word)?);
    }

    Ok(instances)
}

fn accumulator_from_repr(
    indices: &[(usize, usize)],
    instances: &[Vec<Fr>],
) -> Result<KzgAccumulator, VerifyError> {
    if indices.len() != 4 * LIMBS {
        return Err(VerifyError::InvalidHalo2VerifierKey);
    }
    let mut limbs = Vec::with_capacity(indices.len());
    for (instance_index, row) in indices {
        limbs.push(
            *instances
                .get(*instance_index)
                .and_then(|instance| instance.get(*row))
                .ok_or(VerifyError::InvalidHalo2VerifierKey)?,
        );
    }
    let lhs_x = fq_from_limbs(&limbs[0..LIMBS])?;
    let lhs_y = fq_from_limbs(&limbs[LIMBS..2 * LIMBS])?;
    let rhs_x = fq_from_limbs(&limbs[2 * LIMBS..3 * LIMBS])?;
    let rhs_y = fq_from_limbs(&limbs[3 * LIMBS..4 * LIMBS])?;
    let lhs = Option::<G1Affine>::from(G1Affine::from_xy(lhs_x, lhs_y))
        .ok_or(VerifyError::InvalidHalo2VerifierKey)?;
    let rhs = Option::<G1Affine>::from(G1Affine::from_xy(rhs_x, rhs_y))
        .ok_or(VerifyError::InvalidHalo2VerifierKey)?;
    Ok(KzgAccumulator { lhs, rhs })
}

fn fq_from_limbs(limbs: &[Fr]) -> Result<Fq, VerifyError> {
    let mut bytes = [0u8; 33];
    for (index, limb) in limbs.iter().enumerate() {
        let repr = limb.to_repr();
        let limb_bytes = repr.as_ref();
        if limb_bytes[11..].iter().any(|byte| *byte != 0) {
            return Err(VerifyError::InvalidHalo2FieldElement);
        }
        bytes[index * 11..index * 11 + 11].copy_from_slice(&limb_bytes[..11]);
    }
    if bytes[32] != 0 {
        return Err(VerifyError::InvalidHalo2FieldElement);
    }
    let mut repr = [0u8; 32];
    repr.copy_from_slice(&bytes[..32]);
    Fq::from_repr(repr.into())
        .into_option()
        .ok_or(VerifyError::InvalidHalo2FieldElement)
}

fn decode_g1(encoded: &EncodedG1) -> Result<G1Affine, VerifyError> {
    if encoded.0.len() != crate::BN254_G1_RAW_BYTES {
        return Err(VerifyError::InvalidHalo2VerifierKey);
    }
    G1Affine::from_raw_bytes(&encoded.0).ok_or(VerifyError::InvalidHalo2VerifierKey)
}

fn decode_g2(encoded: &EncodedG2) -> Result<G2Affine, VerifyError> {
    if encoded.0.len() != crate::BN254_G2_RAW_BYTES {
        return Err(VerifyError::InvalidHalo2VerifierKey);
    }
    G2Affine::from_raw_bytes(&encoded.0).ok_or(VerifyError::InvalidHalo2VerifierKey)
}

fn powers(base: Fr, len: usize) -> Vec<Fr> {
    let mut out = Vec::with_capacity(len);
    let mut current = Fr::ONE;
    for _ in 0..len {
        out.push(current);
        current *= base;
    }
    out
}

fn invert(value: Fr) -> Result<Fr, VerifyError> {
    Option::<Fr>::from(value.invert()).ok_or(VerifyError::Halo2VerificationFailed)
}

fn fr_to_be(value: Fr) -> [u8; 32] {
    let mut bytes = [0; 32];
    bytes.copy_from_slice(value.to_repr().as_ref());
    bytes.reverse();
    bytes
}

fn fr_from_be_checked(mut bytes: [u8; 32]) -> Result<Fr, VerifyError> {
    bytes.reverse();
    Fr::from_repr(bytes.into())
        .into_option()
        .ok_or(VerifyError::InvalidHalo2FieldElement)
}

fn fq_from_be_checked(mut bytes: [u8; 32]) -> Result<Fq, VerifyError> {
    bytes.reverse();
    Fq::from_repr(bytes.into())
        .into_option()
        .ok_or(VerifyError::InvalidHalo2FieldElement)
}

fn fr_from_be_mod(bytes: [u8; 32]) -> Fr {
    let mut limbs = [0u64; 4];
    for i in 0..4 {
        let start = 32 - (i + 1) * 8;
        limbs[i] = u64::from_be_bytes(bytes[start..start + 8].try_into().unwrap());
    }
    while ge_limbs(&limbs, &FR_MODULUS) {
        sub_limbs(&mut limbs, &FR_MODULUS);
    }
    let mut repr = [0u8; 32];
    for (index, limb) in limbs.iter().enumerate() {
        repr[index * 8..index * 8 + 8].copy_from_slice(&limb.to_le_bytes());
    }
    Fr::from_repr(repr.into()).unwrap()
}

fn ge_limbs(lhs: &[u64; 4], rhs: &[u64; 4]) -> bool {
    for (lhs, rhs) in lhs.iter().zip(rhs).rev() {
        if lhs > rhs {
            return true;
        }
        if lhs < rhs {
            return false;
        }
    }
    true
}

fn sub_limbs(lhs: &mut [u64; 4], rhs: &[u64; 4]) {
    let mut borrow = 0u128;
    for (lhs, rhs) in lhs.iter_mut().zip(rhs) {
        let subtrahend = *rhs as u128 + borrow;
        if (*lhs as u128) >= subtrahend {
            *lhs = (*lhs as u128 - subtrahend) as u64;
            borrow = 0;
        } else {
            *lhs = ((1u128 << 64) + *lhs as u128 - subtrahend) as u64;
            borrow = 1;
        }
    }
}
