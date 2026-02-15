use sha2::{Digest, Sha256};
use std::{env, process};

/// Verify an OpenVM UPLC proof commitment.
///
/// Recomputes SHA256(program_bytes || result_string) and compares it
/// against the commitment revealed by the zkVM guest.
///
/// Usage:
///   openvm-verify <program_hex> <expected_result> <commitment_hex>
///
/// Example:
///   openvm-verify 010000481501 "Integer(42)" 9182033e...
fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("Usage: {} <program_hex> <expected_result> <commitment_hex>", args[0]);
        eprintln!();
        eprintln!("  program_hex      Hex-encoded UPLC program bytes");
        eprintln!("  expected_result  Expected evaluation result string (e.g. \"Integer(42)\")");
        eprintln!("  commitment_hex   The 32-byte commitment from the proof (hex)");
        process::exit(2);
    }

    let program_hex = &args[1];
    let expected_result = &args[2];
    let commitment_hex = &args[3];

    // Decode program hex to bytes
    let program_bytes = match hex::decode(program_hex) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error: invalid program hex: {}", e);
            process::exit(2);
        }
    };

    // Recompute: SHA256(program_bytes || result_string)
    let mut hasher = Sha256::new();
    hasher.update(&program_bytes);
    hasher.update(expected_result.as_bytes());
    let expected_hash = hex::encode(hasher.finalize());

    // Compare
    println!("Program:         {}", program_hex);
    println!("Expected result: {}", expected_result);
    println!();
    println!("Verifier hash:   {}", expected_hash);
    println!("Proof commitment: {}", commitment_hex);
    println!();

    if expected_hash == *commitment_hex {
        println!("✅ MATCH — proof is valid!");
    } else {
        println!("❌ MISMATCH — proof rejected!");
        process::exit(1);
    }
}
