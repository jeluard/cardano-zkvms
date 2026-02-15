#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod impls;

use alloc::boxed::Box;
use alloc::string::String;
use anyhow::Result;
use core::fmt::Display;

/// Abstraction for UPLC evaluation functionality
pub trait UplcEvaluator {
    /// Evaluate a UPLC program given as hex-encoded bytes
    /// Returns the result as a displayable value
    fn evaluate_program(&self, program_hex: &str) -> Result<Box<dyn Display>>;

    fn version(&self) -> &str;
}

/// Error type for UPLC evaluation
#[derive(Debug, thiserror::Error)]
pub enum UplcError {
    #[error("Program decoding error: {0}")]
    DecodeError(String),
    #[error("Evaluation error: {0}")]
    EvaluationError(String),
    #[error("Result conversion error: {0}")]
    ResultError(String),
}

#[derive(Debug, Clone)]
pub struct EvaluationResult {
    pub result: String,
    pub cost: Option<String>,
}

impl Display for EvaluationResult {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.result)
    }
}

pub fn get_evaluator() -> Box<dyn UplcEvaluator> {
    #[cfg(any(feature = "uplc-turbo", feature = "uplc-turbo-riscv"))]
    {
        Box::new(impls::UplcTurboEvaluator::new())
    }

    #[cfg(all(
        feature = "uplc-aiken",
        not(any(feature = "uplc-turbo", feature = "uplc-turbo-riscv"))
    ))]
    {
        Box::new(impls::UplcAikenEvaluator::new())
    }
}
