use std::{env, fs, process};

use openvm_mcu_verifier_core::{decode_message, ProofEnvelope, VerifierKey};
use openvm_mcu_verifier_core::{
    debug_verify_frontend, decode_portable_halo2_key, is_portable_halo2_key_payload,
    OpenVmEvmHalo2Verifier, Verifier,
};

fn main() {
    let mut args = env::args().skip(1);
    let key_path = match args.next() {
        Some(path) => path,
        None => usage_and_exit(),
    };
    let proof_path = match args.next() {
        Some(path) => path,
        None => usage_and_exit(),
    };
    if args.next().is_some() {
        usage_and_exit();
    }

    let key_bytes = read_bytes(&key_path);
    let proof_bytes = read_bytes(&proof_path);
    let key: VerifierKey = decode_message(&key_bytes).unwrap_or_else(|error| {
        eprintln!("failed to decode verifier key {}: {error}", key_path);
        process::exit(1);
    });
    let proof: ProofEnvelope = decode_message(&proof_bytes).unwrap_or_else(|error| {
        eprintln!("failed to decode proof envelope {}: {error}", proof_path);
        process::exit(1);
    });

    if let Ok(path) = env::var("OPENVM_DUMP_KEY_PAYLOAD") {
        fs::write(&path, &key.payload).unwrap_or_else(|error| {
            eprintln!("failed to write {}: {error}", path);
            process::exit(1);
        });
        println!("dumped key payload to {}", path);
    }

    println!("key payload: {} bytes", key.payload.len());
    println!(
        "key payload prefix: {:02x?}",
        &key.payload[..key.payload.len().min(8)]
    );
    println!(
        "portable key header present: {}",
        is_portable_halo2_key_payload(&key.payload)
    );
    match decode_portable_halo2_key(&key.payload) {
        Ok(compact) => {
            println!("portable key decoded: k={} domain_k={}", compact.k, compact.domain_k);
        }
        Err(error) => {
            println!("portable key decode error: {}", error);
        }
    }
    let (stage, debug_result) = debug_verify_frontend(&key, &proof);
    match debug_result {
        Ok(report) => {
            println!(
                "portable verifier stage: {} (instances={}, preprocessed={})",
                stage, report.instance_count, report.preprocessed_commitments
            );
        }
        Err(error) => {
            println!("portable verifier failed at stage '{}': {}", stage, error);
        }
    }

    let mut verifier = OpenVmEvmHalo2Verifier::default();
    match verifier.verify(&key, &proof) {
        Ok(report) => {
            println!("verified: {}", report.verified);
            println!("public values: {} bytes", report.public_values_len);
        }
        Err(error) => {
            println!("verify error: {}", error);
            process::exit(1);
        }
    }
}

fn read_bytes(path: &str) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| {
        eprintln!("failed to read {}: {error}", path);
        process::exit(1);
    })
}

fn usage_and_exit() -> ! {
    eprintln!("usage: openvm-mcu-device-check <packed-key> <packed-proof>");
    process::exit(2);
}