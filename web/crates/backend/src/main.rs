use actix_cors::Cors;
use actix_files as fs;
use actix_web::{web, App, HttpResponse, HttpServer};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use tempfile::NamedTempFile;
use tokio::process::Command;
use tracing::{error, info};

/// Request body for /api/prove
#[derive(Debug, Deserialize)]
struct ProveRequest {
    /// Hex-encoded flat UPLC program
    program_hex: String,
}

/// Response from /api/prove
///
/// The backend returns raw proof data. All processing (VK construction,
/// proof byte conversion, zstd compression) happens client-side in JS.
#[derive(Debug, Serialize)]
struct ProveResponse {
    /// Whether proof generation succeeded
    success: bool,
    /// SHA256(program_bytes || result_string) as hex
    #[serde(skip_serializing_if = "Option::is_none")]
    commitment: Option<String>,
    /// Raw STARK proof JSON: { "proof": "0x...", "user_public_values": "0x..." }
    #[serde(skip_serializing_if = "Option::is_none")]
    stark_proof_json: Option<serde_json::Value>,
    /// App execution commit hex (from cargo openvm commit)
    #[serde(skip_serializing_if = "Option::is_none")]
    app_exe_commit: Option<String>,
    /// App VM commit hex (from cargo openvm commit)
    #[serde(skip_serializing_if = "Option::is_none")]
    app_vm_commit: Option<String>,
    /// Error message if failed
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Duration in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_secs: Option<f64>,
}

/// Shared application state
struct AppState {
    /// Path to the openvm guest crate root (where cargo openvm commands run)
    guest_dir: PathBuf,
}

/// Helper to create an error ProveResponse
fn prove_error(error: String, commitment: Option<String>, duration: Option<f64>) -> HttpResponse {
    HttpResponse::InternalServerError().json(ProveResponse {
        success: false,
        error: Some(error),
        commitment,
        stark_proof_json: None,
        app_exe_commit: None,
        app_vm_commit: None,
        duration_secs: duration,
    })
}

/// POST /api/prove
///
/// Accepts a UPLC program hex, runs it through the OpenVM guest to generate
/// a STARK proof, and returns the proof + commitment + VK.
///
/// Pipeline: run → prove stark → commit → verify stark → convert for WASM
async fn prove(
    data: web::Data<AppState>,
    body: web::Json<ProveRequest>,
) -> HttpResponse {
    let program_hex = body.program_hex.trim().to_string();
    let start = std::time::Instant::now();

    // Validate hex
    if hex::decode(&program_hex).map_or(true, |b| b.is_empty()) {
        return HttpResponse::BadRequest().json(ProveResponse {
            success: false,
            error: Some(if program_hex.is_empty() {
                "Empty program".into()
            } else {
                format!("Invalid hex: {}", hex::decode(&program_hex).unwrap_err())
            }),
            commitment: None,
            stark_proof_json: None,
            app_exe_commit: None,
            app_vm_commit: None,
            duration_secs: None,
        });
    }

    // Note: VK verification is handled client-side using WASM openvm-verifier module

    // 1. Write input JSON to a temporary file for the OpenVM guest CLI
    let input_json = format!(r#"{{"input":["0x01{}"]}}"#, program_hex);
    let input_file = match NamedTempFile::new() {
        Ok(f) => f,
        Err(e) => {
            error!("Failed to create temp input file: {}", e);
            return prove_error(format!("Failed to create temp file: {}", e), None, None);
        }
    };
    let input_path = input_file.path().to_path_buf();

    if let Err(e) = tokio::fs::write(&input_path, &input_json).await {
        error!("Failed to write input file: {}", e);
        return prove_error(format!("Failed to write input: {}", e), None, None);
    }

    info!("Starting proof generation for program: {}...", &program_hex[..program_hex.len().min(20)]);

    // 2. Run `cargo openvm run` to get the evaluation result quickly
    let input_path_str = input_path.to_str().unwrap_or_default();
    let run_output = match Command::new("cargo")
        .args(["openvm", "run", "--input", input_path_str])
        .current_dir(&data.guest_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => {
            error!("Failed to spawn cargo openvm run: {}", e);
            return prove_error(
                format!("Failed to run guest: {}", e),
                None,
                Some(start.elapsed().as_secs_f64()),
            );
        }
    };

    let run_stdout = String::from_utf8_lossy(&run_output.stdout).to_string();
    let run_stderr = String::from_utf8_lossy(&run_output.stderr).to_string();

    if !run_output.status.success() {
        error!("cargo openvm run failed: {}", run_stderr);
        return prove_error(
            format!("Guest execution failed: {}", run_stderr.lines().last().unwrap_or(&run_stderr)),
            None,
            Some(start.elapsed().as_secs_f64()),
        );
    }

    // Extract commitment from run output
    let commitment_hex = extract_commitment(&run_stdout).or_else(|| extract_commitment(&run_stderr));
    info!("Guest run succeeded. Commitment: {:?}", commitment_hex);

    // 3. Generate STARK proof (includes app proving + aggregation in one step)
    info!("Generating aggregated STARK proof (this may take several minutes)...");
    let proof_file = match NamedTempFile::new() {
        Ok(f) => f,
        Err(e) => {
            error!("Failed to create temp proof file: {}", e);
            return prove_error(
                format!("Failed to create temp file: {}", e),
                commitment_hex,
                Some(start.elapsed().as_secs_f64()),
            );
        }
    };
    let stark_proof_path = proof_file.path().to_path_buf();
    let stark_proof_path_str = stark_proof_path.to_str().unwrap_or_default();
    let stark_prove_output = match Command::new("cargo")
        .args(["openvm", "prove", "stark", "--input", input_path_str,
               "--proof", stark_proof_path_str])
        .current_dir(&data.guest_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => {
            error!("Failed to spawn cargo openvm prove stark: {}", e);
            return prove_error(
                format!("STARK proof generation failed to start: {}", e),
                commitment_hex,
                Some(start.elapsed().as_secs_f64()),
            );
        }
    };

    if !stark_prove_output.status.success() {
        let stderr = String::from_utf8_lossy(&stark_prove_output.stderr);
        error!("cargo openvm prove stark failed: {}", stderr);
        return prove_error(
            format!("STARK proof generation failed: {}", stderr.lines().last().unwrap_or(&stderr)),
            commitment_hex,
            Some(start.elapsed().as_secs_f64()),
        );
    }
    info!("STARK proof generation succeeded in {:.1}s", start.elapsed().as_secs_f64());

    // 4. Get app commits (hex strings for client-side VK construction)
    info!("Getting app execution commits...");
    let workspace_root = data.guest_dir.join("../../..");
    let commit_path = workspace_root.join("target/openvm/release/openvm-guest.commit.json");

    let mut app_exe_commit = None;
    let mut app_vm_commit = None;

    let commit_output = Command::new("cargo")
        .args(["openvm", "commit"])
        .current_dir(&data.guest_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await;

    match &commit_output {
        Err(e) => error!("Failed to run cargo openvm commit: {}", e),
        Ok(o) if !o.status.success() => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            error!("cargo openvm commit failed: {}", stderr);
        }
        Ok(_) => {
            match tokio::fs::read_to_string(&commit_path).await {
                Ok(json_str) => {
                    info!("Commit JSON: {}", json_str.trim());
                    if let Ok(commit_json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                        app_exe_commit = commit_json["app_exe_commit"].as_str().map(String::from);
                        app_vm_commit = commit_json["app_vm_commit"].as_str().map(String::from);
                    }
                }
                Err(e) => error!("Cannot read commit JSON at {:?}: {}", commit_path, e),
            }
        }
    };

    // 5. Read raw STARK proof JSON (client will process it)
    let stark_proof_json = match tokio::fs::read_to_string(&stark_proof_path).await {
        Ok(json_str) => {
            info!("Loaded proof JSON: {} bytes", json_str.len());
            serde_json::from_str::<serde_json::Value>(&json_str).ok()
        }
        Err(e) => {
            error!("Failed to read STARK proof file from {}: {}", stark_proof_path.display(), e);
            None
        }
    };

    let duration = start.elapsed().as_secs_f64();
    info!("Proof generation complete in {:.1}s", duration);

    HttpResponse::Ok().json(ProveResponse {
        success: true,
        commitment: commitment_hex,
        stark_proof_json,
        app_exe_commit,
        app_vm_commit,
        error: None,
        duration_secs: Some(duration),
    })
}

/// Extract the commitment hex from "Execution output: [145, 130, ...]" line
fn extract_commitment(output: &str) -> Option<String> {
    for line in output.lines() {
        if let Some(start) = line.find("Execution output: [") {
            let array_start = start + "Execution output: [".len();
            if let Some(end) = line[array_start..].find(']') {
                let nums_str = &line[array_start..array_start + end];
                let hex: String = nums_str
                    .split(',')
                    .filter_map(|s| s.trim().parse::<u8>().ok())
                    .map(|b| format!("{:02x}", b))
                    .collect();
                if hex.len() == 64 {
                    return Some(hex);
                }
            }
        }
    }
    None
}

/// GET /api/health
async fn health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "service": "openvm-web-backend"
    }))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt::init();

    // Resolve guest directory (the openvm guest crate)
    let guest_dir = std::env::var("OPENVM_GUEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // Default: assume we're run from web/backend/, guest is at ../../
            PathBuf::from("../../")
        })
        .canonicalize()
        .expect("Cannot resolve guest directory. Set OPENVM_GUEST_DIR env var.");

    // Static files directory (the web/ folder)
    let static_dir = std::env::var("OPENVM_STATIC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("../"))
        .canonicalize()
        .expect("Cannot resolve static directory. Set OPENVM_STATIC_DIR env var.");

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);

    info!("OpenVM Web Backend starting");
    info!("  Guest dir:       {}", guest_dir.display());
    info!("  Static dir:      {}", static_dir.display());
    info!("  Port:            {}", port);

    let state = web::Data::new(AppState {
        guest_dir,
    });

    let static_dir_clone = static_dir.clone();

    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header();

        App::new()
            .wrap(cors)
            .app_data(state.clone())
            .app_data(web::JsonConfig::default().limit(10 * 1024 * 1024)) // 10 MB JSON limit
            .route("/api/health", web::get().to(health))
            .route("/api/prove", web::post().to(prove))
            // Serve static files (index.html, pkg/, stark-pkg/) from web/
            .service(fs::Files::new("/", static_dir_clone.clone()).index_file("index.html"))
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}
