mod error;
mod hasher;
mod public_values;
mod types;
mod verifier;

use openvm_stark_backend::keygen::types::MultiStarkVerifyingKey;
use openvm_stark_sdk::config::baby_bear_poseidon2::BabyBearPoseidon2Config as SC;
use types::{VerificationBaseline, VerificationBaselineJson, VersionedVmStarkProof};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
}

/// Verify an OpenVM STARK proof.
///
/// # Arguments
/// * `proof_json` - JSON-serialized `VersionedVmStarkProof`
/// * `agg_vk_bytes` - bitcode-serialized `MultiStarkVerifyingKey`
/// * `baseline_json` - JSON-serialized `VerificationBaselineJson`
///
/// # Returns
/// * `Ok(true)` if proof is valid, `Ok(false)` if invalid, `Err` for deserialization errors
#[wasm_bindgen]
pub fn verify_stark(
    proof_json: &str,
    agg_vk_bytes: &[u8],
    baseline_json: &str,
) -> Result<bool, JsValue> {
    let proof_json: VersionedVmStarkProof = serde_json::from_str(proof_json)
        .map_err(|e| JsValue::from_str(&format!("Failed to deserialize proof JSON: {}", e)))?;
    let proof: VmStarkProof = proof_json
        .try_into()
        .map_err(|e| JsValue::from_str(&format!("Failed to decode versioned proof: {}", e)))?;

    let agg_vk: MultiStarkVerifyingKey<SC> = bitcode::deserialize(agg_vk_bytes).map_err(|e| {
        JsValue::from_str(&format!(
            "Failed to deserialize aggregation verification key: {}",
            e
        ))
    })?;
    let baseline_json: VerificationBaselineJson = serde_json::from_str(baseline_json)
        .map_err(|e| JsValue::from_str(&format!("Failed to deserialize verification baseline: {}", e)))?;
    let baseline: VerificationBaseline = baseline_json.into();

    match verifier::verify_vm_stark_proof_decoded(&agg_vk, &baseline, &proof) {
        Ok(()) => Ok(true),
        Err(e) => {
            log(&format!("OpenVM verification failed: {}", e));
            Ok(false)
        }
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

use types::VmStarkProof;
