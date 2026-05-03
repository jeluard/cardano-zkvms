//! Host-side OpenVM SDK wrapper for building, executing, proving, and key
//! generation — replaces `cargo openvm` CLI commands with direct Rust API calls.

use std::path::Path;

use eyre::{Result, WrapErr};
use openvm_circuit::arch::instructions::exe::VmExe;
use openvm_continuations::CommitBytes;
use openvm_sdk::config::{AggregationSystemParams, AppConfig};
use openvm_sdk::fs::{read_object_from_file, write_object_to_file};
#[cfg(feature = "evm-prove")]
use openvm_sdk::keygen::Halo2ProvingKey;
use openvm_sdk::keygen::{AggProvingKey, AppProvingKey};
#[cfg(feature = "evm-prove")]
use openvm_sdk::types::EvmProof;
use openvm_sdk::types::{VerificationBaselineJson, VersionedVmStarkProof};
use openvm_sdk::{Sdk, StdIn};
use openvm_sdk_config::SdkVmConfig;
use openvm_stark_backend::{keygen::types::MultiStarkVerifyingKey, SystemParams};
use openvm_stark_sdk::config::{app_params_with_100_bits_security, MAX_APP_LOG_STACKED_HEIGHT};

// Re-export crates used by downstream consumers (e.g. the web backend).
pub use openvm_circuit;
pub use openvm_sdk;
pub use openvm_sdk::types::VerificationBaselineJson as StarkVerificationBaselineJson;

pub type F = openvm_sdk::F;

/// Convenience type aliases for downstream crates.
pub type Config = AppConfig<SdkVmConfig>;
pub type Exe = VmExe<F>;
pub type AppPk = AppProvingKey<SdkVmConfig>;
pub type AggPk = AggProvingKey;
pub type AggVk = MultiStarkVerifyingKey<openvm_sdk::SC>;
#[cfg(feature = "evm-prove")]
pub type Halo2Pk = Halo2ProvingKey;

/// Result of a STARK proof generation.
pub struct StarkProveResult {
    /// Serialized STARK proof as JSON (for sending to client).
    pub proof_json: serde_json::Value,
    /// Baseline artifacts needed by the 2.0 verifier.
    pub baseline_json: VerificationBaselineJson,
    /// OpenVM proof format version.
    pub proof_version: String,
    /// App execution commit hex string.
    pub app_exe_commit: String,
    /// App VM commit hex string.
    pub app_vm_commit: String,
}

fn default_app_system_params() -> SystemParams {
    app_params_with_100_bits_security(MAX_APP_LOG_STACKED_HEIGHT)
}

fn default_agg_params() -> AggregationSystemParams {
    AggregationSystemParams::default()
}

fn sdk_from_config(config: Config) -> Result<Sdk> {
    Sdk::new(config, default_agg_params()).map_err(Into::into)
}

fn sdk_from_keys(app_pk: AppPk, agg_pk: AggPk) -> Result<Sdk> {
    Sdk::builder()
        .app_pk(app_pk)
        .agg_pk(agg_pk)
        .build()
        .map_err(Into::into)
}

fn commit_hex(commit: CommitBytes) -> String {
    format!("0x{}", hex::encode(commit.as_slice()))
}

pub fn openvm_version() -> &'static str {
    openvm_sdk::OPENVM_VERSION
}

/// Load an `AppConfig` from an `openvm.toml` file.
pub fn load_config(config_path: &Path) -> Result<AppConfig<SdkVmConfig>> {
    let toml_str = std::fs::read_to_string(config_path)
        .wrap_err_with(|| format!("Failed to read config: {}", config_path.display()))?;
    let vm_config = SdkVmConfig::from_toml(&toml_str).wrap_err("Failed to parse openvm.toml")?;
    Ok(AppConfig::new(vm_config, default_app_system_params()))
}

/// Load a pre-built guest executable (`.vmexe`) from disk.
pub fn load_exe(vmexe_path: &Path) -> Result<VmExe<F>> {
    read_object_from_file(vmexe_path)
        .wrap_err_with(|| format!("Failed to load vmexe: {}", vmexe_path.display()))
}

/// Load app proving key from disk.
pub fn load_app_pk(pk_path: &Path) -> Result<AppProvingKey<SdkVmConfig>> {
    read_object_from_file(pk_path)
        .wrap_err_with(|| format!("Failed to load app proving key: {}", pk_path.display()))
}

/// Load aggregation proving key from disk.
pub fn load_agg_pk(agg_pk_path: &Path) -> Result<AggProvingKey> {
    read_object_from_file(agg_pk_path)
        .wrap_err_with(|| format!("Failed to load agg proving key: {}", agg_pk_path.display()))
}

/// Load aggregation verifying key from disk.
pub fn load_agg_vk(agg_vk_path: &Path) -> Result<AggVk> {
    read_object_from_file(agg_vk_path).wrap_err_with(|| {
        format!(
            "Failed to load agg verifying key: {}",
            agg_vk_path.display()
        )
    })
}

/// Build the guest crate → ELF → VmExe, equivalent to `cargo openvm build`.
///
/// This cross-compiles the guest to riscv32im and transpiles the ELF to a VmExe.
pub fn build_guest(manifest_path: &Path, config_path: &Path, target_dir: &Path) -> Result<()> {
    let config = load_config(config_path)?;
    let sdk = sdk_from_config(config)?;

    let guest_opts = Default::default();
    let pkg_dir = manifest_path
        .parent()
        .ok_or_else(|| eyre::eyre!("Invalid manifest path"))?;
    let target_filter = Default::default();

    let elf = sdk
        .build(guest_opts, pkg_dir, &target_filter, None)
        .wrap_err("Failed to build guest ELF")?;
    let exe = sdk
        .convert_to_exe(elf)
        .wrap_err("Failed to convert ELF to VmExe")?;

    let vmexe_dir = target_dir.join("openvm/release");
    std::fs::create_dir_all(&vmexe_dir)?;
    let vmexe_path = vmexe_dir.join("openvm-guest.vmexe");
    write_object_to_file(&vmexe_path, exe.as_ref())
        .wrap_err_with(|| format!("Failed to write vmexe to {}", vmexe_path.display()))?;

    tracing::info!("Guest built: {}", vmexe_path.display());
    Ok(())
}

/// Generate app proving key, equivalent to `cargo openvm keygen`.
///
/// Note: the SDK also produces an `AppVerifyingKey` but we don't persist it —
/// the client reconstructs the full VK from `agg_stark.vk` + per-program commits.
pub fn generate_app_pk(config_path: &Path, target_dir: &Path) -> Result<()> {
    let config = load_config(config_path)?;
    let sdk = sdk_from_config(config)?;

    let (app_pk, _app_vk) = sdk.app_keygen();

    let openvm_dir = target_dir.join("openvm");
    std::fs::create_dir_all(&openvm_dir)?;

    let pk_path = openvm_dir.join("app.pk");
    write_object_to_file(&pk_path, &app_pk).wrap_err("Failed to write app.pk")?;

    tracing::info!("App proving key generated: {}", pk_path.display());
    Ok(())
}

/// Generate aggregation proving key + verifying key, equivalent to `cargo openvm setup`.
pub fn generate_agg_keys(config_path: &Path, openvm_home: &Path) -> Result<()> {
    let config = load_config(config_path)?;
    let sdk = sdk_from_config(config)?;

    let (agg_pk, agg_vk) = sdk.agg_keygen();

    std::fs::create_dir_all(openvm_home)?;

    let pk_path = openvm_home.join("agg_stark.pk");
    write_object_to_file(&pk_path, &agg_pk).wrap_err("Failed to write agg_stark.pk")?;

    let vk_path = openvm_home.join("agg_stark.vk");
    write_object_to_file(&vk_path, &agg_vk).wrap_err("Failed to write agg_stark.vk")?;

    tracing::info!("Aggregation keys generated in {}", openvm_home.display());
    Ok(())
}

/// Build StdIn from raw program bytes for the guest.
///
/// The guest expects `openvm::io::read_vec()` to return the program bytes.
fn make_stdin(program_bytes: &[u8]) -> StdIn {
    let mut stdin = StdIn::default();
    stdin.write_bytes(program_bytes);
    stdin
}

/// Execute the guest without proof generation (fast).
///
/// Returns the user public values (32-byte SHA256 commitment).
/// Equivalent to `cargo openvm run`.
pub fn execute(
    config: &AppConfig<SdkVmConfig>,
    exe: &VmExe<F>,
    program_bytes: &[u8],
) -> Result<Vec<u8>> {
    let stdin = make_stdin(program_bytes);
    let sdk = sdk_from_config(config.clone())?;
    let output = sdk
        .execute(exe.clone(), stdin)
        .wrap_err("Guest execution failed")?;
    Ok(output)
}

/// Generate a STARK proof for the given program.
///
/// Equivalent to `cargo openvm prove stark` + `cargo openvm commit`.
/// Returns the proof JSON, commits, and public values in one call.
pub fn prove_stark(
    exe: &VmExe<F>,
    app_pk: &AppProvingKey<SdkVmConfig>,
    agg_pk: &AggProvingKey,
    program_bytes: &[u8],
) -> Result<StarkProveResult> {
    let stdin = make_stdin(program_bytes);

    let sdk = sdk_from_keys(app_pk.clone(), agg_pk.clone())?;
    let mut prover = sdk
        .prover(exe.clone())
        .wrap_err("Failed to create STARK prover")?;
    let (proof, _) = prover
        .prove(stdin, &[])
        .wrap_err("STARK proof generation failed")?;
    let baseline = prover.generate_baseline();
    let baseline_json = VerificationBaselineJson::from(baseline.clone());
    let app_exe_commit = commit_hex(baseline_json.app_exe_commit);
    let app_vm_commit = CommitBytes::from(prover.app_vm_commit());

    let versioned =
        VersionedVmStarkProof::new(proof).wrap_err("Failed to create versioned proof")?;
    let proof_version = versioned.version.clone();
    let proof_json =
        serde_json::to_value(&versioned).wrap_err("Failed to serialize proof to JSON")?;

    Ok(StarkProveResult {
        proof_json,
        baseline_json,
        proof_version,
        app_exe_commit,
        app_vm_commit: commit_hex(app_vm_commit),
    })
}

/// Verify a STARK proof using the native OpenVM 2.0 verifier.
pub fn verify_stark(
    agg_vk: &AggVk,
    proof_json: &serde_json::Value,
    baseline_json: &VerificationBaselineJson,
) -> Result<()> {
    let versioned: VersionedVmStarkProof = serde_json::from_value(proof_json.clone())
        .wrap_err("Failed to deserialize versioned proof JSON")?;
    let proof = versioned
        .try_into()
        .wrap_err("Failed to decode STARK proof")?;

    Sdk::verify_proof(agg_vk.clone(), baseline_json.clone().into(), &proof)
        .wrap_err("STARK proof verification failed")?;
    Ok(())
}

// =============================================================================
// EVM / Halo2 proving (behind `evm-prove` feature flag)
// =============================================================================

/// Generate Halo2 proving key for EVM proof wrapping.
///
/// **Warning:** Requires >64 GB RAM and takes 10+ minutes.
/// The resulting key is >10 GB serialized. Run once and persist.
#[cfg(feature = "evm-prove")]
pub fn generate_halo2_pk(
    config: &AppConfig<SdkVmConfig>,
    app_pk: &AppProvingKey<SdkVmConfig>,
    agg_pk: &AggProvingKey,
) -> Result<Halo2ProvingKey> {
    let _ = config;
    let sdk = sdk_from_keys(app_pk.clone(), agg_pk.clone())?;
    Ok(sdk.halo2_pk())
}

/// Generate an EVM-verifiable Halo2/KZG proof by wrapping a STARK proof.
///
/// This runs the full pipeline: STARK prove → aggregation → root → Halo2 wrapper.
/// Much slower than `prove_stark` and requires Halo2 keys.
#[cfg(feature = "evm-prove")]
pub fn prove_evm(
    exe: &VmExe<F>,
    app_pk: &AppProvingKey<SdkVmConfig>,
    agg_pk: &AggProvingKey,
    halo2_pk: &Halo2ProvingKey,
    program_bytes: &[u8],
) -> Result<EvmProof> {
    let stdin = make_stdin(program_bytes);

    let sdk = Sdk::builder()
        .app_pk(app_pk.clone())
        .agg_pk(agg_pk.clone())
        .halo2_pk(halo2_pk.clone())
        .build()
        .wrap_err("Failed to initialize EVM proving SDK")?;

    let evm_proof = sdk
        .prove_evm(exe.clone(), stdin, &[])
        .wrap_err("EVM Halo2 proof generation failed")?;

    Ok(evm_proof)
}

/// Compute app execution commits without generating a proof.
///
/// Equivalent to `cargo openvm commit`.
pub fn compute_app_commit(
    exe: &VmExe<F>,
    app_pk: &AppProvingKey<SdkVmConfig>,
    agg_pk: &AggProvingKey,
) -> Result<(String, String)> {
    let sdk = sdk_from_keys(app_pk.clone(), agg_pk.clone())?;
    let prover = sdk
        .prover(exe.clone())
        .wrap_err("Failed to create STARK prover")?;
    let baseline = prover.generate_baseline();
    let app_vm_commit = CommitBytes::from(prover.app_vm_commit());

    Ok((
        commit_hex(CommitBytes::from(baseline.app_exe_commit)),
        commit_hex(app_vm_commit),
    ))
}

#[cfg(feature = "evm-prove")]
pub mod evm_halo2_mcu {
    use std::{env, fs, io::BufReader, path::PathBuf};

    use eyre::{eyre, Context, Result};
    use openvm_sdk::{
        fs::read_object_from_file,
        keygen::{Halo2ProvingKey, RootProvingKey},
        types::EvmProof,
        Sdk,
    };
    use serde::{Deserialize, Serialize};
    use snark_verifier_sdk::snark_verifier::{
        halo2_base::halo2_proofs::{
            halo2curves::bn256::{Bn256, Fr, G1Affine},
            plonk::verify_proof,
            poly::{
                commitment::{Params, ParamsProver},
                kzg::{
                    commitment::{KZGCommitmentScheme, ParamsKZG},
                    multiopen::VerifierSHPLONK,
                    strategy::AccumulatorStrategy,
                },
                VerificationStrategy,
            },
            transcript::TranscriptReadBuffer,
        },
        loader::native::NativeLoader,
        system::halo2::transcript::evm::EvmTranscript,
    };
    use snark_verifier_sdk::{
        halo2::aggregation::AggregationCircuit,
        snark_verifier::{
            pcs::kzg::KzgDecidingKey,
            system::halo2::{compile, Config},
            verifier::plonk::PlonkProtocol,
        },
        CircuitExt,
    };

    use crate::{make_stdin, AggPk, AppPk, Exe};

    const BN254_BYTES: usize = 32;
    const NUM_ACCUMULATOR: usize = 12;

    pub struct McuHalo2Artifacts {
        pub proof: EvmProof,
        pub proof_json: serde_json::Value,
        pub native_verifier_key: Vec<u8>,
    }

    pub fn prove_mcu_halo2(
        exe: &Exe,
        app_pk: &AppPk,
        agg_pk: &AggPk,
        program_bytes: &[u8],
    ) -> Result<McuHalo2Artifacts> {
        let mut builder = Sdk::builder().app_pk(app_pk.clone()).agg_pk(agg_pk.clone());

        if let Some(root_pk_path) = configured_path(&["OPENVM_ROOT_PK", "MCU_EVM_ROOT_PK"]) {
            let root_pk: RootProvingKey = read_object_from_file(&root_pk_path).with_context(|| {
                format!(
                    "failed to load root proving key: {}",
                    root_pk_path.display()
                )
            })?;
            builder = builder.root_pk(root_pk);
        }

        if let Some(halo2_pk_path) = configured_path(&["OPENVM_HALO2_PK", "MCU_EVM_HALO2_PK"]) {
            let halo2_pk: Halo2ProvingKey = read_object_from_file(&halo2_pk_path).with_context(|| {
                format!(
                    "failed to load Halo2 proving key: {}",
                    halo2_pk_path.display()
                )
            })?;
            builder = builder.halo2_pk(halo2_pk);
        }

        let sdk = builder.build().wrap_err("failed to initialize OpenVM SDK")?;

        let proof = sdk
            .prove_evm(exe.clone(), make_stdin(program_bytes), &[])
            .wrap_err("failed to generate OpenVM Halo2/KZG proof")?;

        verify_halo2_kzg_native(&sdk, proof.clone())
            .wrap_err("native Halo2/KZG verifier rejected generated proof")?;

        let proof_json =
            serde_json::to_value(&proof).wrap_err("failed to serialize Halo2 proof")?;
        let native_verifier_key = native_verifier_key_bytes(&sdk.halo2_pk())?;

        Ok(McuHalo2Artifacts {
            proof,
            proof_json,
            native_verifier_key,
        })
    }

    fn configured_path(names: &[&str]) -> Option<PathBuf> {
        names
            .iter()
            .find_map(|name| env::var(name).ok().filter(|value| !value.is_empty()))
            .map(PathBuf::from)
    }

    fn verify_halo2_kzg_native(sdk: &Sdk, proof: EvmProof) -> Result<()> {
        let halo2_pk = sdk.halo2_pk();
        let raw_proof = decode_openvm_halo2_proof(proof)?;
        let params = read_kzg_params(halo2_pk.wrapper.pinning.metadata.config_params.k)?;

        let instances = [raw_proof.instances.as_slice()];
        let proof_batches = [instances.as_slice()];
        let verifier_params = params.verifier_params();
        let mut transcript =
            EvmTranscript::<G1Affine, NativeLoader, _, _>::init(raw_proof.proof.as_slice());
        let strategy = verify_proof::<
            KZGCommitmentScheme<Bn256>,
            VerifierSHPLONK<_>,
            _,
            EvmTranscript<_, _, _, _>,
            _,
        >(
            verifier_params,
            halo2_pk.wrapper.pinning.pk.get_vk(),
            AccumulatorStrategy::new(verifier_params),
            &proof_batches,
            &mut transcript,
        )
        .map_err(|error| eyre!("native Halo2 verifier failed: {error:?}"))?;

        if VerificationStrategy::<_, VerifierSHPLONK<_>>::finalize(strategy) {
            Ok(())
        } else {
            Err(eyre!("native KZG accumulator decision rejected proof"))
        }
    }

    struct NativeRawProof {
        instances: Vec<Fr>,
        proof: Vec<u8>,
    }

    fn decode_openvm_halo2_proof(proof: EvmProof) -> Result<NativeRawProof> {
        let accumulator = proof.proof_data.accumulator;
        let halo2_proof = proof.proof_data.proof;
        if accumulator.len() != NUM_ACCUMULATOR * BN254_BYTES {
            return Err(eyre!(
                "invalid KZG accumulator length: {}",
                accumulator.len()
            ));
        }
        if halo2_proof.len() != 43 * BN254_BYTES {
            return Err(eyre!("invalid Halo2 proof length: {}", halo2_proof.len()));
        }

        let mut instance_words = Vec::new();
        for chunk in accumulator.chunks_exact(BN254_BYTES) {
            let mut word = [0; BN254_BYTES];
            word.copy_from_slice(chunk);
            word.reverse();
            instance_words.push(word);
        }

        let mut app_exe_commit = [0; BN254_BYTES];
        app_exe_commit.copy_from_slice(proof.app_commit.app_exe_commit.as_slice());
        app_exe_commit.reverse();
        instance_words.push(app_exe_commit);

        let mut app_vm_commit = [0; BN254_BYTES];
        app_vm_commit.copy_from_slice(proof.app_commit.app_vm_commit.as_slice());
        app_vm_commit.reverse();
        instance_words.push(app_vm_commit);

        for byte in proof.user_public_values {
            let mut word = [0; BN254_BYTES];
            word[0] = byte;
            instance_words.push(word);
        }

        let instances = instance_words
            .iter()
            .map(|word| {
                Option::<Fr>::from(Fr::from_bytes(word))
                    .ok_or_else(|| eyre!("invalid BN254 scalar instance"))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(NativeRawProof {
            instances,
            proof: halo2_proof,
        })
    }

    fn read_kzg_params(k: usize) -> Result<ParamsKZG<Bn256>> {
        let params_dir = env::var("OPENVM_KZG_PARAMS_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|_| {
                env::var("HOME").map(|home| std::path::PathBuf::from(home).join(".openvm/params"))
            })
            .wrap_err("set OPENVM_KZG_PARAMS_DIR or HOME so native KZG params can be loaded")?;
        let path = params_dir.join(format!("kzg_bn254_{k}.srs"));
        let file = fs::File::open(&path)
            .with_context(|| format!("failed to open KZG params: {}", path.display()))?;
        ParamsKZG::<Bn256>::read(&mut BufReader::new(file))
            .map_err(|error| eyre!("failed to read KZG params {}: {error:?}", path.display()))
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct NativeHalo2VerifierKey {
        protocol: PlonkProtocol<G1Affine>,
        deciding_key: KzgDecidingKey<Bn256>,
        num_pvs: Vec<usize>,
        k: usize,
    }

    fn native_verifier_key_bytes(halo2_pk: &Halo2ProvingKey) -> Result<Vec<u8>> {
        let k = halo2_pk.wrapper.pinning.metadata.config_params.k;
        let params = read_kzg_params(k)?;
        let num_pvs = halo2_pk.wrapper.pinning.metadata.num_pvs.clone();
        let protocol = compile(
            &params,
            halo2_pk.wrapper.pinning.pk.get_vk(),
            Config::kzg()
                .with_num_instance(num_pvs.clone())
                .with_accumulator_indices(AggregationCircuit::accumulator_indices()),
        );
        let deciding_key = (params.get_g()[0], params.g2(), params.s_g2()).into();
        let native_key = NativeHalo2VerifierKey {
            protocol,
            deciding_key,
            num_pvs,
            k,
        };
        bincode::serialize(&native_key).wrap_err("failed to serialize native verifier key")
    }
}
