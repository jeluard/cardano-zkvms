//! Host-side OpenVM SDK wrapper for building, executing, proving, and key
//! generation — replaces `cargo openvm` CLI commands with direct Rust API calls.

use std::path::Path;
use std::sync::Arc;

use eyre::{Result, WrapErr};
use openvm_circuit::arch::instructions::exe::VmExe;
use openvm_sdk::config::{AppConfig, SdkVmConfig};
use openvm_sdk::fs::{read_object_from_file, write_object_to_file};
use openvm_sdk::keygen::{AggProvingKey, AggVerifyingKey, AppProvingKey};
#[cfg(feature = "evm-prove")]
use openvm_sdk::keygen::Halo2ProvingKey;
use openvm_sdk::types::VersionedVmStarkProof;
#[cfg(feature = "evm-prove")]
use openvm_sdk::types::EvmProof;
use openvm_sdk::{Sdk, StdIn};

// Re-export crates used by downstream consumers (e.g. the web backend).
pub use openvm_circuit;
pub use openvm_sdk;

pub type F = openvm_sdk::F;

/// Convenience type aliases for downstream crates.
pub type Config = AppConfig<SdkVmConfig>;
pub type Exe = VmExe<F>;
pub type AppPk = AppProvingKey<SdkVmConfig>;
pub type AggPk = AggProvingKey;
pub type AggVk = AggVerifyingKey;
#[cfg(feature = "evm-prove")]
pub type Halo2Pk = Halo2ProvingKey;

/// Result of a STARK proof generation.
pub struct StarkProveResult {
    /// Serialized STARK proof as JSON (for sending to client).
    pub proof_json: serde_json::Value,
    /// App execution commit hex string.
    pub app_exe_commit: String,
    /// App VM commit hex string.
    pub app_vm_commit: String,
    /// User public values (the 32-byte commitment revealed by the guest).
    pub public_values: Vec<u8>,
}

/// Load an `AppConfig` from an `openvm.toml` file.
pub fn load_config(config_path: &Path) -> Result<AppConfig<SdkVmConfig>> {
    let toml_str = std::fs::read_to_string(config_path)
        .wrap_err_with(|| format!("Failed to read config: {}", config_path.display()))?;
    let config = SdkVmConfig::from_toml(&toml_str)
        .wrap_err("Failed to parse openvm.toml")?;
    Ok(config)
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
pub fn load_agg_vk(agg_vk_path: &Path) -> Result<AggVerifyingKey> {
    read_object_from_file(agg_vk_path)
        .wrap_err_with(|| format!("Failed to load agg verifying key: {}", agg_vk_path.display()))
}

/// Build the guest crate → ELF → VmExe, equivalent to `cargo openvm build`.
///
/// This cross-compiles the guest to riscv32im and transpiles the ELF to a VmExe.
pub fn build_guest(
    manifest_path: &Path,
    config_path: &Path,
    target_dir: &Path,
) -> Result<()> {
    let config = load_config(config_path)?;
    let sdk = Sdk::new(config)?;

    let guest_opts = Default::default();
    let pkg_dir = manifest_path
        .parent()
        .ok_or_else(|| eyre::eyre!("Invalid manifest path"))?;
    let target_filter = Default::default();

    let elf = sdk.build(guest_opts, pkg_dir, &target_filter, None)
        .wrap_err("Failed to build guest ELF")?;
    let exe = sdk.convert_to_exe(elf)
        .wrap_err("Failed to convert ELF to VmExe")?;

    let vmexe_dir = target_dir.join("openvm/release");
    std::fs::create_dir_all(&vmexe_dir)?;
    let vmexe_path = vmexe_dir.join("openvm-guest.vmexe");
    write_object_to_file(&vmexe_path, &exe)
        .wrap_err_with(|| format!("Failed to write vmexe to {}", vmexe_path.display()))?;

    tracing::info!("Guest built: {}", vmexe_path.display());
    Ok(())
}

/// Generate app proving key, equivalent to `cargo openvm keygen`.
///
/// Note: the SDK also produces an `AppVerifyingKey` but we don't persist it —
/// the client reconstructs the full VK from `agg_stark.vk` + per-program commits.
pub fn generate_app_pk(
    config_path: &Path,
    target_dir: &Path,
) -> Result<()> {
    let config = load_config(config_path)?;
    let sdk = Sdk::new(config)?;

    let (app_pk, _app_vk) = sdk.app_keygen();

    let openvm_dir = target_dir.join("openvm");
    std::fs::create_dir_all(&openvm_dir)?;

    let pk_path = openvm_dir.join("app.pk");
    write_object_to_file(&pk_path, &app_pk)
        .wrap_err("Failed to write app.pk")?;

    tracing::info!("App proving key generated: {}", pk_path.display());
    Ok(())
}

/// Generate aggregation proving key + verifying key, equivalent to `cargo openvm setup`.
pub fn generate_agg_keys(openvm_home: &Path) -> Result<()> {
    let sdk = Sdk::standard();

    let (agg_pk, agg_vk) = sdk.agg_keygen()
        .wrap_err("Failed to generate aggregation keys")?;

    std::fs::create_dir_all(openvm_home)?;

    let pk_path = openvm_home.join("agg_stark.pk");
    write_object_to_file(&pk_path, &agg_pk)
        .wrap_err("Failed to write agg_stark.pk")?;

    let vk_path = openvm_home.join("agg_stark.vk");
    write_object_to_file(&vk_path, &agg_vk)
        .wrap_err("Failed to write agg_stark.vk")?;

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
    let sdk = Sdk::new(config.clone())?;
    let output = sdk.execute(Arc::new(exe.clone()), stdin)
        .wrap_err("Guest execution failed")?;
    Ok(output)
}

/// Generate a STARK proof for the given program.
///
/// Equivalent to `cargo openvm prove stark` + `cargo openvm commit`.
/// Returns the proof JSON, commits, and public values in one call.
pub fn prove_stark(
    config: &AppConfig<SdkVmConfig>,
    exe: &VmExe<F>,
    app_pk: &AppProvingKey<SdkVmConfig>,
    agg_pk: &AggProvingKey,
    program_bytes: &[u8],
) -> Result<StarkProveResult> {
    let stdin = make_stdin(program_bytes);

    let sdk = Sdk::new(config.clone())?
        .with_app_pk(app_pk.clone())
        .with_agg_pk(agg_pk.clone());

    let (proof, commit) = sdk.prove(Arc::new(exe.clone()), stdin)
        .wrap_err("STARK proof generation failed")?;

    let versioned = VersionedVmStarkProof::new(proof)
        .wrap_err("Failed to create versioned proof")?;
    let proof_json = serde_json::to_value(&versioned)
        .wrap_err("Failed to serialize proof to JSON")?;

    Ok(StarkProveResult {
        proof_json,
        app_exe_commit: format!("{}", commit.app_exe_commit),
        app_vm_commit: format!("{}", commit.app_vm_commit),
        public_values: versioned.user_public_values.to_vec(),
    })
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
    let sdk = Sdk::new(config.clone())?
        .with_app_pk(app_pk.clone())
        .with_agg_pk(agg_pk.clone());
    let halo2_pk = sdk.halo2_keygen();
    Ok(halo2_pk)
}

/// Generate an EVM-verifiable Halo2/KZG proof by wrapping a STARK proof.
///
/// This runs the full pipeline: STARK prove → aggregation → root → Halo2 wrapper.
/// Much slower than `prove_stark` and requires Halo2 keys.
#[cfg(feature = "evm-prove")]
pub fn prove_evm(
    config: &AppConfig<SdkVmConfig>,
    exe: &VmExe<F>,
    app_pk: &AppProvingKey<SdkVmConfig>,
    agg_pk: &AggProvingKey,
    halo2_pk: &Halo2ProvingKey,
    program_bytes: &[u8],
) -> Result<EvmProof> {
    let stdin = make_stdin(program_bytes);

    let sdk = Sdk::new(config.clone())?
        .with_app_pk(app_pk.clone())
        .with_agg_pk(agg_pk.clone())
        .with_halo2_pk(halo2_pk.clone());

    let evm_proof = sdk.prove_evm(Arc::new(exe.clone()), stdin)
        .wrap_err("EVM Halo2 proof generation failed")?;

    Ok(evm_proof)
}

/// Compute app execution commits without generating a proof.
///
/// Equivalent to `cargo openvm commit`.
pub fn compute_app_commit(
    config: &AppConfig<SdkVmConfig>,
    exe: &VmExe<F>,
    app_pk: &AppProvingKey<SdkVmConfig>,
) -> Result<(String, String)> {
    let sdk = Sdk::new(config.clone())?
        .with_app_pk(app_pk.clone());
    let prover = sdk.app_prover(Arc::new(exe.clone()))
        .wrap_err("Failed to create app prover")?;
    let commit = prover.app_commit();

    Ok((
        format!("{}", commit.app_exe_commit),
        format!("{}", commit.app_vm_commit),
    ))
}
