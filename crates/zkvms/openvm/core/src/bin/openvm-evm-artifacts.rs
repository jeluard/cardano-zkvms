use std::{env, fs, io::BufReader, path::PathBuf};

use eyre::{eyre, Context, Result};
use openvm_prover::{load_agg_pk, load_app_pk, load_config, load_exe};
use openvm_sdk::{
    config::AggregationSystemParams,
    fs::{read_object_from_file, write_object_to_file, write_to_file_json},
    keygen::{Halo2ProvingKey, RootProvingKey},
    types::EvmProof,
    Sdk, StdIn,
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

const BN254_BYTES: usize = 32;
const NUM_ACCUMULATOR: usize = 12;

#[derive(Debug, Default)]
struct Args {
    config: Option<PathBuf>,
    vmexe: PathBuf,
    program: PathBuf,
    proof_json: PathBuf,
    verifier_dir: PathBuf,
    app_pk: Option<PathBuf>,
    agg_pk: Option<PathBuf>,
    root_pk: Option<PathBuf>,
    halo2_pk: Option<PathBuf>,
    write_root_pk: Option<PathBuf>,
    write_halo2_pk: Option<PathBuf>,
    skip_local_verify: bool,
}

fn usage() -> &'static str {
    "usage: openvm-evm-artifacts \
      --vmexe <openvm-guest.vmexe> \
      --program <uplc-bytes> \
      --proof-json <evm-proof.json> \
    --verifier-dir <halo2-native-verifier-dir> \
      [--config <openvm.toml> | --app-pk <app.pk>] \
      [--agg-pk <agg_stark.pk>] \
      [--root-pk <root.pk>] \
      [--halo2-pk <halo2.pk>] \
      [--write-root-pk <root.pk>] \
      [--write-halo2-pk <halo2.pk>] \
      [--skip-local-verify]"
}

fn parse_args() -> Result<Args> {
    let mut args = Args::default();
    let mut raw_args = env::args().skip(1);

    while let Some(flag) = raw_args.next() {
        let mut take_value = || {
            raw_args
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| eyre!("missing value for {flag}"))
        };

        match flag.as_str() {
            "--config" => args.config = Some(take_value()?),
            "--vmexe" => args.vmexe = take_value()?,
            "--program" => args.program = take_value()?,
            "--proof-json" => args.proof_json = take_value()?,
            "--verifier-dir" => args.verifier_dir = take_value()?,
            "--app-pk" => args.app_pk = Some(take_value()?),
            "--agg-pk" => args.agg_pk = Some(take_value()?),
            "--root-pk" => args.root_pk = Some(take_value()?),
            "--halo2-pk" => args.halo2_pk = Some(take_value()?),
            "--write-root-pk" => args.write_root_pk = Some(take_value()?),
            "--write-halo2-pk" => args.write_halo2_pk = Some(take_value()?),
            "--skip-local-verify" => args.skip_local_verify = true,
            "--help" | "-h" => {
                println!("{}", usage());
                std::process::exit(0);
            }
            other => return Err(eyre!("unknown argument {other}\n{}", usage())),
        }
    }

    if args.vmexe.as_os_str().is_empty()
        || args.program.as_os_str().is_empty()
        || args.proof_json.as_os_str().is_empty()
        || args.verifier_dir.as_os_str().is_empty()
    {
        return Err(eyre!(usage()));
    }

    if args.config.is_none() && args.app_pk.is_none() {
        return Err(eyre!("provide either --config or --app-pk\n{}", usage()));
    }

    if args.halo2_pk.is_some() && args.root_pk.is_none() {
        return Err(eyre!(
            "--halo2-pk requires --root-pk because OpenVM ties Halo2 keys to the root verifier key"
        ));
    }

    if args.root_pk.is_some() && (args.app_pk.is_none() || args.agg_pk.is_none()) {
        return Err(eyre!("--root-pk requires --app-pk and --agg-pk"));
    }

    Ok(args)
}

fn make_stdin(program_bytes: &[u8]) -> StdIn {
    let mut stdin = StdIn::default();
    stdin.write_bytes(program_bytes);
    stdin
}

fn main() -> Result<()> {
    let args = parse_args()?;

    let exe = load_exe(&args.vmexe)?;
    let program_bytes = fs::read(&args.program)
        .with_context(|| format!("failed to read program bytes: {}", args.program.display()))?;

    let mut builder = Sdk::builder();

    if let Some(app_pk_path) = &args.app_pk {
        builder = builder.app_pk(load_app_pk(app_pk_path)?);
    } else {
        let config_path = args.config.as_ref().expect("validated config or app pk");
        builder = builder.app_config(load_config(config_path)?);
    }

    if let Some(agg_pk_path) = &args.agg_pk {
        builder = builder.agg_pk(load_agg_pk(agg_pk_path)?);
    } else {
        builder = builder.agg_params(AggregationSystemParams::default());
    }

    if let Some(root_pk_path) = &args.root_pk {
        let root_pk: RootProvingKey = read_object_from_file(root_pk_path).with_context(|| {
            format!(
                "failed to load root proving key: {}",
                root_pk_path.display()
            )
        })?;
        builder = builder.root_pk(root_pk);
    }

    if let Some(halo2_pk_path) = &args.halo2_pk {
        let halo2_pk: Halo2ProvingKey =
            read_object_from_file(halo2_pk_path).with_context(|| {
                format!(
                    "failed to load Halo2 proving key: {}",
                    halo2_pk_path.display()
                )
            })?;
        builder = builder.halo2_pk(halo2_pk);
    }

    let sdk = builder
        .build()
        .wrap_err("failed to initialize OpenVM SDK")?;

    if let Some(root_pk_path) = &args.write_root_pk {
        write_object_to_file(root_pk_path, &sdk.root_pk()).with_context(|| {
            format!(
                "failed to write root proving key: {}",
                root_pk_path.display()
            )
        })?;
    }

    let evm_proof = sdk
        .prove_evm(exe, make_stdin(&program_bytes), &[])
        .wrap_err("failed to generate OpenVM EVM Halo2/KZG proof")?;
    write_to_file_json(&args.proof_json, &evm_proof)
        .with_context(|| format!("failed to write proof JSON: {}", args.proof_json.display()))?;

    if let Some(halo2_pk_path) = &args.write_halo2_pk {
        write_object_to_file(halo2_pk_path, &sdk.halo2_pk()).with_context(|| {
            format!(
                "failed to write Halo2 proving key: {}",
                halo2_pk_path.display()
            )
        })?;
    }

    if !args.skip_local_verify {
        verify_halo2_kzg_native(&sdk, evm_proof.clone())
            .wrap_err("native Halo2/KZG verifier rejected the generated proof")?;
        eprintln!("native Halo2/KZG verification succeeded");
    }

    write_native_verifier_artifacts(&args.verifier_dir, &sdk.halo2_pk())?;

    Ok(())
}

fn verify_halo2_kzg_native(sdk: &Sdk, evm_proof: EvmProof) -> Result<()> {
    let halo2_pk = sdk.halo2_pk();
    let raw_proof = decode_openvm_halo2_proof(evm_proof)?;
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

fn decode_openvm_halo2_proof(evm_proof: EvmProof) -> Result<NativeRawProof> {
    let accumulator = evm_proof.proof_data.accumulator;
    let proof = evm_proof.proof_data.proof;
    if accumulator.len() != NUM_ACCUMULATOR * BN254_BYTES {
        return Err(eyre!(
            "invalid KZG accumulator length: {}",
            accumulator.len()
        ));
    }
    if proof.len() != 43 * BN254_BYTES {
        return Err(eyre!("invalid Halo2 proof length: {}", proof.len()));
    }

    let mut instance_words = Vec::new();
    for chunk in accumulator.chunks_exact(BN254_BYTES) {
        let mut word = [0; BN254_BYTES];
        word.copy_from_slice(chunk);
        word.reverse();
        instance_words.push(word);
    }

    let mut app_exe_commit = [0; BN254_BYTES];
    app_exe_commit.copy_from_slice(evm_proof.app_commit.app_exe_commit.as_slice());
    app_exe_commit.reverse();
    instance_words.push(app_exe_commit);

    let mut app_vm_commit = [0; BN254_BYTES];
    app_vm_commit.copy_from_slice(evm_proof.app_commit.app_vm_commit.as_slice());
    app_vm_commit.reverse();
    instance_words.push(app_vm_commit);

    for byte in evm_proof.user_public_values {
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

    Ok(NativeRawProof { instances, proof })
}

fn read_kzg_params(k: usize) -> Result<ParamsKZG<Bn256>> {
    let params_dir = env::var("OPENVM_KZG_PARAMS_DIR")
        .map(PathBuf::from)
        .or_else(|_| env::var("HOME").map(|home| PathBuf::from(home).join(".openvm/params")))
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

fn write_native_verifier_artifacts(
    verifier_dir: &PathBuf,
    halo2_pk: &Halo2ProvingKey,
) -> Result<()> {
    fs::create_dir_all(verifier_dir)
        .with_context(|| format!("failed to create {}", verifier_dir.display()))?;
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
        num_pvs: num_pvs.clone(),
        k,
    };
    let key_path = verifier_dir.join("native-verifier.bin");
    let key_bytes =
        bincode::serialize(&native_key).wrap_err("failed to serialize native verifier key")?;
    fs::write(&key_path, key_bytes).with_context(|| {
        format!(
            "failed to write native verifier key: {}",
            key_path.display()
        )
    })?;

    let manifest = serde_json::json!({
        "kind": "openvm-halo2-kzg-native",
        "verification": "native-halo2-kzg",
        "key_file": key_path,
        "num_pvs": num_pvs,
        "k": k
    });
    let manifest_path = verifier_dir.join("native-verifier.json");
    write_to_file_json(&manifest_path, &manifest).with_context(|| {
        format!(
            "failed to write native verifier manifest: {}",
            manifest_path.display()
        )
    })
}
