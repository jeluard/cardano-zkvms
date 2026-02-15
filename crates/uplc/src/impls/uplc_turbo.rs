use alloc::boxed::Box;
use alloc::format;
use crate::{UplcError, UplcEvaluator};
use super::{decode_program_hex, make_result};

#[cfg(all(feature = "uplc-turbo", not(feature = "uplc-turbo-riscv")))]
use uplc_turbo;
#[cfg(feature = "uplc-turbo-riscv")]
use uplc_turbo_riscv as uplc_turbo;

use uplc_turbo::{arena::Arena, binder::DeBruijn, flat};
pub struct UplcTurboEvaluator;

impl UplcTurboEvaluator {
    pub fn new() -> Self {
        Self
    }
}

impl UplcEvaluator for UplcTurboEvaluator {
    fn evaluate_program(&self, program_hex: &str) -> anyhow::Result<Box<dyn core::fmt::Display>> {
        let program_bytes = decode_program_hex(program_hex)?;

        let arena = Arena::new();

        let program: &uplc_turbo::program::Program<DeBruijn> = flat::decode(&arena, &program_bytes)
            .map_err(|e| UplcError::DecodeError(format!("Program decode error: {:?}", e)))?;

        let eval_result = program.eval(&arena);

        let result_term = eval_result
            .term
            .map_err(|e| UplcError::EvaluationError(format!("Evaluation error: {:?}", e)))?;

        let result_constant = match &result_term {
            uplc_turbo::term::Term::Constant(c) => c,
            _ => {
                return Err(UplcError::ResultError(
                    "Evaluation result is not a constant".into(),
                )
                .into());
            }
        };

        let result = make_result(
            format!("{:?}", result_constant),
            Some(format!("{:?}", eval_result.info.consumed_budget)),
        );

        Ok(Box::new(result))
    }

    fn version(&self) -> &str {
        "uplc-turbo"
    }
}
