use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=OPENVM_MCU_RP2350_KEY");
    println!("cargo:rerun-if-env-changed=OPENVM_MCU_RP2350_PROOF");
    println!("cargo:rerun-if-env-changed=OPENVM_MCU_RP2350_PROOF_SHA");
    println!("cargo:rerun-if-env-changed=OPENVM_MCU_RP2350_VISUAL_ONLY");
    println!("cargo:rerun-if-changed=/tmp/openvm-mcu-rp2350-compact.key");
    println!("cargo:rerun-if-changed=/tmp/openvm-mcu-rp2350.proof");

    write_embedded_artifacts();

    if env::var_os("CARGO_FEATURE_BLE_TRANSFER").is_some()
        && env::var_os("CARGO_FEATURE_SKIP_CYW43_FIRMWARE").is_none()
    {
        download_cyw43_firmware();
    }

    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("arm") {
        println!(
            "cargo:rustc-link-search={}",
            PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir")).display()
        );
        println!("cargo:rustc-link-arg=-Tlink.x");
        println!("cargo:rustc-link-arg=-Trp2350-image.x");
        println!("cargo:rustc-link-arg=--nmagic");
        println!("cargo:rerun-if-changed=memory.x");
        println!("cargo:rerun-if-changed=rp2350-image.x");
    }
}

fn download_cyw43_firmware() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let download_dir = manifest_dir.join("cyw43-firmware");
    let url_base = "https://github.com/embassy-rs/embassy/raw/refs/heads/main/cyw43-firmware";
    let files = [
        "43439A0.bin",
        "43439A0_btfw.bin",
        "43439A0_clm.bin",
        "nvram_rp2040.bin",
        "LICENSE-permissive-binary-license-1.0.txt",
        "README.md",
    ];

    println!("cargo:rerun-if-changed={}", download_dir.display());
    fs::create_dir_all(&download_dir).expect("create cyw43 firmware directory");

    for file in files {
        let destination = download_dir.join(file);
        if destination.exists() {
            continue;
        }
        let url = format!("{url_base}/{file}");
        let status = Command::new("curl")
            .args(["--fail", "--location", "--silent", "--show-error", "--output"])
            .arg(&destination)
            .arg(&url)
            .status()
            .unwrap_or_else(|error| panic!("failed to start curl for {url}: {error}"));
        if !status.success() {
            panic!("failed to download {url} to {}", destination.display());
        }
    }
}

fn write_embedded_artifacts() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let generated = out_dir.join("embedded_artifacts.rs");

    let key = env::var("OPENVM_MCU_RP2350_KEY")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| default_artifact("/tmp/openvm-mcu-rp2350-compact.key"));
    let proof = env::var("OPENVM_MCU_RP2350_PROOF")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| default_artifact("/tmp/openvm-mcu-rp2350.proof"));
    let sha = env::var("OPENVM_MCU_RP2350_PROOF_SHA").unwrap_or_else(|_| "embedded".into());

    let source = match (key, proof) {
        (Some(key), Some(proof)) => {
            let key = PathBuf::from(key);
            let proof = PathBuf::from(proof);
            if !key.exists() {
                panic!("OPENVM_MCU_RP2350_KEY does not exist: {}", key.display());
            }
            if !proof.exists() {
                panic!("OPENVM_MCU_RP2350_PROOF does not exist: {}", proof.display());
            }
            println!("cargo:rerun-if-changed={}", key.display());
            println!("cargo:rerun-if-changed={}", proof.display());
            format!(
                "pub fn artifacts() -> Option<(&'static [u8], &'static [u8], &'static str)> {{\n    Some((include_bytes!({:?}).as_slice(), include_bytes!({:?}).as_slice(), {:?}))\n}}\n",
                key.display().to_string(),
                proof.display().to_string(),
                sha
            )
        }
        _ => "pub fn artifacts() -> Option<(&'static [u8], &'static [u8], &'static str)> {\n    None\n}\n".into(),
    };

    fs::write(generated, source).expect("write embedded artifact module");
}

fn default_artifact(path: &str) -> Option<String> {
    let path = PathBuf::from(path);
    if path.exists() {
        println!("cargo:rerun-if-changed={}", path.display());
        Some(path.display().to_string())
    } else {
        None
    }
}
