use sha2::{Digest, Sha256};

openvm::entry!(main);

pub fn main() {
    // Read flat-encoded UPLC program from host
    let program_bytes: Vec<u8> = openvm::io::read_vec();

    if program_bytes.is_empty() {
        panic!("No program provided");
    }

    // Convert program bytes to hex string for the evaluator
    let program_hex = hex::encode(&program_bytes);

    // Create evaluator and evaluate the program
    // The evaluator implementation is selected based on enabled features
    let evaluator = uplc::get_evaluator();

    match evaluator.evaluate_program(&program_hex) {
        Ok(result) => {
            // Hash program bytes + evaluation result together.
            // This commits the proof to BOTH the input program AND its output,
            // so a verifier can confirm "program X produced result Y".
            let result_str = result.to_string();
            let mut hasher = Sha256::new();
            hasher.update(&program_bytes);
            hasher.update(result_str.as_bytes());
            let commitment: [u8; 32] = hasher.finalize().into();

            // Reveal the combined hash as the public output of the proof
            openvm::io::reveal_bytes32(commitment);
        }
        Err(e) => {
            panic!("UPLC evaluation failed: {}", e);
        }
    }
}
