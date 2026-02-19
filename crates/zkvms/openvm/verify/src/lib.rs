use verify_stark::{verify_vm_stark_proof, vk::VmStarkVerifyingKey};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
}

/// Verify an OpenVM STARK proof.
///
/// # Arguments
/// * `proof_bytes` - zstd-compressed proof data (proof || user_public_values)
/// * `vk_bytes` - bitcode-serialized `VmStarkVerifyingKey`
///
/// # Returns
/// * `Ok(true)` if proof is valid, `Ok(false)` if invalid, `Err` for deserialization errors
#[wasm_bindgen]
pub fn verify_stark(proof_bytes: &[u8], vk_bytes: &[u8]) -> Result<bool, JsValue> {
    let vk: VmStarkVerifyingKey = bitcode::deserialize(vk_bytes)
        .map_err(|e| JsValue::from_str(&format!("Failed to deserialize verification key: {}", e)))?;

    match verify_vm_stark_proof(&vk, proof_bytes) {
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
