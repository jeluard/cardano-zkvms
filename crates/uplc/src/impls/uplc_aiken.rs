#[cfg(feature = "uplc-aiken")]
use super::{decode_program_hex, make_result};
#[cfg(feature = "uplc-aiken")]
use crate::{UplcError, UplcEvaluator};
#[cfg(feature = "uplc-aiken")]
use alloc::boxed::Box;
#[cfg(feature = "uplc-aiken")]
use alloc::format;
#[cfg(feature = "uplc-aiken")]
use uplc_aiken::ast::{DeBruijn, Program, Term};
#[cfg(feature = "uplc-aiken")]
use uplc_aiken::machine::cost_model::ExBudget;

#[cfg(feature = "uplc-aiken")]
#[derive(Default)]
pub struct UplcAikenEvaluator;

#[cfg(feature = "uplc-aiken")]
impl UplcAikenEvaluator {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(feature = "uplc-aiken")]
impl UplcEvaluator for UplcAikenEvaluator {
    fn evaluate_program(&self, program_hex: &str) -> anyhow::Result<Box<dyn core::fmt::Display>> {
        let program_bytes = decode_program_hex(program_hex)?;

        let program = Program::<DeBruijn>::from_flat(&program_bytes)
            .map_err(|e| UplcError::DecodeError(format!("Program decode error: {:?}", e)))?;

        let budget = ExBudget::default();
        let eval_result = program.eval(budget);
        let cost = eval_result.cost();

        let result_term = eval_result
            .result()
            .map_err(|e| UplcError::EvaluationError(format!("Evaluation error: {:?}", e)))?;

        let result_constant = match &result_term {
            Term::Constant(c) => c,
            _ => {
                return Err(
                    UplcError::ResultError("Evaluation result is not a constant".into()).into(),
                );
            }
        };

        let result = make_result(result_constant.to_pretty(), Some(format!("{:?}", cost)));

        Ok(Box::new(result))
    }

    fn version(&self) -> &str {
        "uplc-aiken"
    }
}
