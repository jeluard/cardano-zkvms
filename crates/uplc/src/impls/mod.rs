#[cfg(feature = "uplc-aiken")]
pub mod uplc_aiken;

#[cfg(any(feature = "uplc-turbo", feature = "uplc-turbo-riscv"))]
pub mod uplc_turbo;

#[cfg(any(feature = "uplc-turbo", feature = "uplc-turbo-riscv"))]
pub use uplc_turbo::UplcTurboEvaluator;

#[cfg(feature = "uplc-aiken")]
pub use self::uplc_aiken::UplcAikenEvaluator;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::UplcError;

/// Decode a hex-encoded UPLC program into raw bytes.
pub fn decode_program_hex(program_hex: &str) -> Result<Vec<u8>, UplcError> {
    hex::decode(program_hex.trim())
        .map_err(|e| UplcError::DecodeError(format!("Hex decode error: {}", e)))
}

/// Build an `EvaluationResult` from a result string and optional cost string.
pub fn make_result(result: String, cost: Option<String>) -> crate::EvaluationResult {
    crate::EvaluationResult { result, cost }
}
