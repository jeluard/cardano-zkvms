use wasm_bindgen::prelude::*;
use sha2::{Digest, Sha256};
use uplc_turbo::{arena::Arena, binder::DeBruijn, flat};

/// Initialize panic hook for better error messages in the browser console.
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

/// Evaluate a hex-encoded flat UPLC program and return the result string.
///
/// Returns the result as a string like "Integer(42)".
#[wasm_bindgen]
pub fn evaluate_uplc(program_hex: &str) -> Result<String, JsValue> {
    let program_bytes = hex::decode(program_hex.trim())
        .map_err(|e| JsValue::from_str(&format!("Hex decode error: {}", e)))?;

    let arena = Arena::new();

    let program: &uplc_turbo::program::Program<DeBruijn> =
        flat::decode(&arena, &program_bytes)
            .map_err(|e| JsValue::from_str(&format!("Program decode error: {:?}", e)))?;

    let eval_result = program.eval(&arena);

    let result_term = eval_result
        .term
        .map_err(|e| JsValue::from_str(&format!("Evaluation error: {:?}", e)))?;

    let result_str = match &result_term {
        uplc_turbo::term::Term::Constant(c) => format!("{:?}", c),
        other => format!("{:?}", other),
    };

    Ok(result_str)
}

/// Compute SHA256(program_bytes || result_string) — the same commitment
/// that the OpenVM guest reveals as public output.
///
/// Returns the 64-char hex digest.
#[wasm_bindgen]
pub fn compute_commitment(program_hex: &str, result_str: &str) -> Result<String, JsValue> {
    let program_bytes = hex::decode(program_hex.trim())
        .map_err(|e| JsValue::from_str(&format!("Hex decode error: {}", e)))?;

    let mut hasher = Sha256::new();
    hasher.update(&program_bytes);
    hasher.update(result_str.as_bytes());
    let hash = hasher.finalize();

    Ok(hex::encode(hash))
}

/// Convert a hex-encoded flat UPLC program to human-readable form.
///
/// Returns a formatted display of the program structure.
#[wasm_bindgen]
pub fn hex_to_uplc(program_hex: &str) -> Result<String, JsValue> {
    let program_bytes = hex::decode(program_hex.trim())
        .map_err(|e| JsValue::from_str(&format!("Hex decode error: {}", e)))?;

    let arena = Arena::new();

    let program: &uplc_turbo::program::Program<DeBruijn> =
        flat::decode(&arena, &program_bytes)
            .map_err(|e| JsValue::from_str(&format!("Program decode error: {:?}", e)))?;

    Ok(format!("{:#?}", program))
}
