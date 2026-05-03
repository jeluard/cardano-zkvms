use std::{fs, path::PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use openvm_mcu_verifier_core::{
    debug_compact_halo2_key_from_native_payload, decode_frame, decode_message, encode_frame,
    encode_message, FrameType, OpenVmEvmHalo2Verifier, ProofEnvelope, ProofKind, Verifier,
    VerifierKey, OPENVM_EVM_HALO2_PROOF_DATA_LEN,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Parser)]
#[command(author, version, about = "OpenVM MCU artifact tooling")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    PackEvmProof {
        #[arg(long)]
        proof_json: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        key_file: Option<PathBuf>,
        #[arg(long)]
        frame: bool,
    },
    PackStarkProof {
        #[arg(long)]
        prove_response_json: PathBuf,
        #[arg(long)]
        key_file: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        frame: bool,
    },
    PackStarkKey {
        #[arg(long)]
        agg_vk_file: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        frame: bool,
    },
    PackKey {
        #[arg(long)]
        key_file: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        frame: bool,
    },
    PackCompactKey {
        #[arg(long)]
        native_key_file: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        frame: bool,
    },
    InspectProof {
        #[arg(long)]
        proof: PathBuf,
    },
    VerifyEvm {
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        proof: PathBuf,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::PackEvmProof {
            proof_json,
            out,
            key_file,
            frame,
        } => {
            let key_id = match key_file {
                Some(path) => key_id_from_file(&path)?,
                None => [0; 32],
            };
            let envelope = read_evm_proof_json(&proof_json, key_id)?;
            let bytes = if frame {
                encode_frame(FrameType::ProofEnvelope, &envelope)?
            } else {
                encode_message(&envelope)?
            };
            fs::write(&out, bytes).with_context(|| format!("write {}", out.display()))?;
            println!("packed proof envelope: {}", out.display());
        }
        Command::PackKey {
            key_file,
            out,
            frame,
        } => {
            let payload = fs::read(&key_file)
                .with_context(|| format!("read verifier key payload {}", key_file.display()))?;
            let key = VerifierKey::new(ProofKind::OpenVmEvmHalo2, hash32(&payload), payload);
            let bytes = if frame {
                encode_frame(FrameType::VerifierKey, &key)?
            } else {
                encode_message(&key)?
            };
            fs::write(&out, bytes).with_context(|| format!("write {}", out.display()))?;
            println!("packed verifier key: {}", out.display());
        }
        Command::PackCompactKey {
            native_key_file,
            out,
            frame,
        } => {
            let native_payload = fs::read(&native_key_file).with_context(|| {
                format!(
                    "read native verifier key payload {}",
                    native_key_file.display()
                )
            })?;
            let key_id = hash32(&native_payload);
            let payload = match debug_compact_halo2_key_from_native_payload(&native_payload) {
                (_, Ok(payload)) => payload,
                (stage, Err(error)) => {
                    bail!("compact key failed at {stage}: {error}");
                }
            };
            let key = VerifierKey::new(ProofKind::OpenVmEvmHalo2, key_id, payload);
            let bytes = if frame {
                encode_frame(FrameType::VerifierKey, &key)?
            } else {
                encode_message(&key)?
            };
            fs::write(&out, bytes).with_context(|| format!("write {}", out.display()))?;
            println!("packed compact verifier key: {}", out.display());
        }
        Command::PackStarkProof {
            prove_response_json,
            key_file,
            out,
            frame,
        } => {
            let key_id = key_id_from_file(&key_file)?;
            let envelope = read_stark_prove_response_json(&prove_response_json, key_id)?;
            let bytes = if frame {
                encode_frame(FrameType::ProofEnvelope, &envelope)?
            } else {
                encode_message(&envelope)?
            };
            fs::write(&out, bytes).with_context(|| format!("write {}", out.display()))?;
            println!("packed STARK proof envelope: {}", out.display());
        }
        Command::PackStarkKey {
            agg_vk_file,
            out,
            frame,
        } => {
            let payload = fs::read(&agg_vk_file)
                .with_context(|| format!("read aggregation VK {}", agg_vk_file.display()))?;
            let key = VerifierKey::new(ProofKind::OpenVmStark, hash32(&payload), payload);
            let bytes = if frame {
                encode_frame(FrameType::VerifierKey, &key)?
            } else {
                encode_message(&key)?
            };
            fs::write(&out, bytes).with_context(|| format!("write {}", out.display()))?;
            println!("packed STARK verifier key: {}", out.display());
        }
        Command::InspectProof { proof } => {
            let bytes = fs::read(&proof).with_context(|| format!("read {}", proof.display()))?;
            let envelope: ProofEnvelope = decode_artifact(&bytes, FrameType::ProofEnvelope)?;
            println!("protocol version: {}", envelope.protocol_version);
            println!("proof kind: {:?}", envelope.proof_kind);
            println!("openvm version: {}", envelope.openvm_version);
            println!("public values: {} bytes", envelope.user_public_values.len());
            println!("proof data: {} bytes", envelope.proof_data.len());
        }
        Command::VerifyEvm { key, proof } => {
            let key_bytes = fs::read(&key).with_context(|| format!("read {}", key.display()))?;
            let proof_bytes =
                fs::read(&proof).with_context(|| format!("read {}", proof.display()))?;
            let key: VerifierKey = decode_artifact(&key_bytes, FrameType::VerifierKey)?;
            let proof: ProofEnvelope = decode_artifact(&proof_bytes, FrameType::ProofEnvelope)?;

            let mut verifier = OpenVmEvmHalo2Verifier::default();
            let report = verifier.verify(&key, &proof)?;
            println!("verified: {}", report.verified);
            println!("public values: {} bytes", report.public_values_len);
        }
    }
    Ok(())
}

fn decode_artifact<'bytes, T>(bytes: &'bytes [u8], expected_frame_type: FrameType) -> Result<T>
where
    T: serde::Deserialize<'bytes>,
{
    if bytes.starts_with(&openvm_mcu_verifier_core::FRAME_MAGIC) {
        let frame = decode_frame(bytes)?;
        if frame.frame_type != expected_frame_type {
            bail!(
                "unexpected frame type {:?}, expected {:?}",
                frame.frame_type,
                expected_frame_type
            );
        }
        decode_message(frame.payload).map_err(Into::into)
    } else {
        decode_message(bytes).map_err(Into::into)
    }
}

fn read_evm_proof_json(path: &PathBuf, key_id: [u8; 32]) -> Result<ProofEnvelope> {
    let json: Value = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("read proof JSON {}", path.display()))?,
    )
    .with_context(|| format!("parse proof JSON {}", path.display()))?;

    let version = json
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let app_exe_commit = hex32(required_str(&json, "app_exe_commit")?)?;
    let app_vm_commit = hex32(required_str(&json, "app_vm_commit")?)?;
    let user_public_values = hex_vec(required_str(&json, "user_public_values")?)?;
    let proof_data = json.get("proof_data").context("missing proof_data")?;
    let mut packed_proof_data = hex_vec(required_str(proof_data, "accumulator")?)?;
    packed_proof_data.extend(hex_vec(required_str(proof_data, "proof")?)?);

    if packed_proof_data.len() != OPENVM_EVM_HALO2_PROOF_DATA_LEN {
        bail!(
            "invalid packed proof_data length: got {}, expected {}",
            packed_proof_data.len(),
            OPENVM_EVM_HALO2_PROOF_DATA_LEN
        );
    }

    Ok(ProofEnvelope::new_evm_halo2(
        version,
        key_id,
        app_exe_commit,
        app_vm_commit,
        user_public_values,
        packed_proof_data,
    ))
}

fn read_stark_prove_response_json(path: &PathBuf, key_id: [u8; 32]) -> Result<ProofEnvelope> {
    let json: Value = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("read prove response JSON {}", path.display()))?,
    )
    .with_context(|| format!("parse prove response JSON {}", path.display()))?;

    if json.get("success").and_then(Value::as_bool) == Some(false) {
        bail!("prove response is not successful: {}", json);
    }

    let openvm_version = json
        .get("openvm_version")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let app_exe_commit = hex32(required_str(&json, "app_exe_commit")?)?;
    let app_vm_commit = hex32(required_str(&json, "app_vm_commit")?)?;
    let user_public_values = hex_vec(required_str(&json, "commitment")?)?;
    let proof_json = json
        .get("stark_proof_json")
        .context("missing stark_proof_json")?;
    let baseline_json = json
        .get("verification_baseline_json")
        .context("missing verification_baseline_json")?;

    Ok(ProofEnvelope::new_stark(
        openvm_version,
        key_id,
        app_exe_commit,
        app_vm_commit,
        user_public_values,
        serde_json::to_vec(proof_json)?,
        serde_json::to_vec(baseline_json)?,
    ))
}

fn required_str<'json>(value: &'json Value, field: &str) -> Result<&'json str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .with_context(|| format!("missing string field {field}"))
}

fn hex_vec(value: &str) -> Result<Vec<u8>> {
    let hex = value.strip_prefix("0x").unwrap_or(value);
    Ok(hex::decode(hex)?)
}

fn hex32(value: &str) -> Result<[u8; 32]> {
    let bytes = hex_vec(value)?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("expected 32-byte hex value"))
}

fn key_id_from_file(path: &PathBuf) -> Result<[u8; 32]> {
    let payload = fs::read(path).with_context(|| format!("read key file {}", path.display()))?;
    Ok(hash32(&payload))
}

fn hash32(payload: &[u8]) -> [u8; 32] {
    Sha256::digest(payload).into()
}
