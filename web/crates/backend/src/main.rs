use actix_cors::Cors;
use actix_web::{web, App, HttpResponse, HttpServer};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{error, info};

/// Resolve the OpenVM home directory (~/.openvm or OPENVM_HOME).
fn openvm_home() -> PathBuf {
    std::env::var("OPENVM_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/root"))
                .join(".openvm")
        })
}

/// `cardano-zkvms setup` — one-time provisioning: build guest, keygen, agg keygen.
fn cmd_setup() -> eyre::Result<()> {
    let guest_dir = std::env::var("OPENVM_GUEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("../../crates/zkvms/openvm"));

    let guest_dir = guest_dir
        .canonicalize()
        .expect("Cannot resolve guest directory. Set OPENVM_GUEST_DIR env var.");

    let workspace_root = guest_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .expect("Guest dir must be 3 levels deep from workspace root")
        .to_path_buf();

    let config_path = guest_dir.join("openvm.toml");
    let manifest_path = guest_dir.join("guest/Cargo.toml");
    let target_dir = workspace_root.join("target");
    let openvm_home = openvm_home();

    // Step 1: Build guest
    let vmexe_path = target_dir.join("openvm/release/openvm-guest.vmexe");
    if vmexe_path.exists() {
        eprintln!("[1/3] Guest vmexe already exists, skipping build");
    } else {
        eprintln!("[1/3] Building guest...");
        openvm_prover::build_guest(&manifest_path, &config_path, &target_dir)?;
        eprintln!("  Done.");
    }

    // Step 2: Generate app proving key
    let pk_path = target_dir.join("openvm/app.pk");
    if pk_path.exists() {
        eprintln!("[2/3] App proving key already exists, skipping keygen");
    } else {
        eprintln!("[2/3] Generating app proving key...");
        openvm_prover::generate_app_pk(&config_path, &target_dir)?;
        eprintln!("  Done.");
    }

    // Step 3: Generate aggregation keys
    let agg_pk_path = openvm_home.join("agg_stark.pk");
    if agg_pk_path.exists() {
        eprintln!("[3/3] Aggregation keys already exist, skipping setup");
    } else {
        eprintln!("[3/3] Generating aggregation keys (this may take 30+ minutes)...");
        openvm_prover::generate_agg_keys(&openvm_home)?;
        eprintln!("  Done.");
    }

    eprintln!("Setup complete.");
    Ok(())
}

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
    /// App execution commit hex (from app commit)
    #[serde(skip_serializing_if = "Option::is_none")]
    app_exe_commit: Option<String>,
    /// App VM commit hex (from app commit)
    #[serde(skip_serializing_if = "Option::is_none")]
    app_vm_commit: Option<String>,
    /// Error message if failed
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Duration in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_secs: Option<f64>,
}

/// Shared application state — holds pre-loaded OpenVM artifacts.
///
/// All OpenVM keys and config are loaded once at startup and reused across
/// requests. Proof generation is CPU-bound and runs via `web::block()`.
struct AppState {
    /// Pre-loaded OpenVM config, executable, and keys.
    config: openvm_prover::Config,
    exe: openvm_prover::Exe,
    app_pk: openvm_prover::AppPk,
    agg_pk: openvm_prover::AggPk,
    /// OpenVM home directory (~/.openvm) — contains agg_stark.vk.
    openvm_home: PathBuf,
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
/// a STARK proof, and returns the proof + commitment.
///
/// Pipeline (all via SDK API, no subprocess calls):
///   1. Execute guest (fast) → get commitment
///   2. Generate STARK proof (slow) → proof + commits
async fn prove(
    data: web::Data<AppState>,
    body: web::Json<ProveRequest>,
) -> HttpResponse {
    let program_hex = body.program_hex.trim().to_string();
    let start = std::time::Instant::now();

    // Validate hex
    let program_bytes = match hex::decode(&program_hex) {
        Ok(b) if !b.is_empty() => b,
        Ok(_) => {
            return HttpResponse::BadRequest().json(ProveResponse {
                success: false,
                error: Some("Empty program".into()),
                commitment: None,
                stark_proof_json: None,
                app_exe_commit: None,
                app_vm_commit: None,
                duration_secs: None,
            });
        }
        Err(e) => {
            return HttpResponse::BadRequest().json(ProveResponse {
                success: false,
                error: Some(format!("Invalid hex: {}", e)),
                commitment: None,
                stark_proof_json: None,
                app_exe_commit: None,
                app_vm_commit: None,
                duration_secs: None,
            });
        }
    };

    info!(
        "Starting proof generation for program: {}...",
        &program_hex[..program_hex.len().min(20)]
    );

    // Clone what we need for the blocking task.
    let config = data.config.clone();
    let exe = data.exe.clone();
    let app_pk = data.app_pk.clone();
    let agg_pk = data.agg_pk.clone();

    // Run the entire pipeline in a blocking thread (CPU-bound work).
    let result = web::block(move || -> Result<ProveResponse, String> {
        // 1. Execute guest (fast) to validate program and get commitment
        info!("Executing guest (validation run)...");
        let output = openvm_prover::execute(&config, &exe, &program_bytes)
            .map_err(|e| format!("Guest execution failed: {}", e))?;

        let commitment_hex = if output.len() == 32 {
            Some(hex::encode(&output))
        } else {
            None
        };
        info!("Guest executed. Commitment: {:?}", commitment_hex);

        // 2. Generate STARK proof (slow — minutes)
        info!("Generating STARK proof (this may take several minutes)...");
        let prove_result =
            openvm_prover::prove_stark(&config, &exe, &app_pk, &agg_pk, &program_bytes)
                .map_err(|e| format!("STARK proof generation failed: {}", e))?;

        let duration = start.elapsed().as_secs_f64();
        info!("STARK proof generated in {:.1}s", duration);

        Ok(ProveResponse {
            success: true,
            commitment: commitment_hex,
            stark_proof_json: Some(prove_result.proof_json),
            app_exe_commit: Some(prove_result.app_exe_commit),
            app_vm_commit: Some(prove_result.app_vm_commit),
            error: None,
            duration_secs: Some(duration),
        })
    })
    .await;

    match result {
        Ok(Ok(response)) => HttpResponse::Ok().json(response),
        Ok(Err(e)) => {
            error!("Prove pipeline error: {}", e);
            prove_error(e, None, Some(start.elapsed().as_secs_f64()))
        }
        Err(e) => {
            error!("Blocking task error: {}", e);
            prove_error(
                format!("Internal error: {}", e),
                None,
                Some(start.elapsed().as_secs_f64()),
            )
        }
    }
}

/// GET /data/agg_stark.vk
///
/// Serve the aggregation STARK verifying key from the OpenVM home directory
/// (~/.openvm/agg_stark.vk).
async fn serve_agg_stark_vk(data: web::Data<AppState>) -> HttpResponse {
    let vk_path = data.openvm_home.join("agg_stark.vk");
    match tokio::fs::read(&vk_path).await {
        Ok(bytes) => {
            info!(
                "Serving agg_stark.vk: {} bytes from {}",
                bytes.len(),
                vk_path.display()
            );
            HttpResponse::Ok()
                .content_type("application/octet-stream")
                .append_header(("Cache-Control", "public, max-age=86400"))
                .body(bytes)
        }
        Err(e) => {
            error!(
                "Failed to read agg_stark.vk from {}: {}",
                vk_path.display(),
                e
            );
            HttpResponse::NotFound().json(serde_json::json!({
                "error": format!(
                    "agg_stark.vk not found at {}. Run 'cardano-zkvms setup' on the server.",
                    vk_path.display()
                )
            }))
        }
    }
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

    // Dispatch: `cardano-zkvms setup` runs one-time provisioning, otherwise serve.
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "setup" => {
                if let Err(e) = cmd_setup() {
                    eprintln!("Setup failed: {:?}", e);
                    std::process::exit(1);
                }
                return Ok(());
            }
            other => {
                eprintln!("Unknown command: {}", other);
                eprintln!("Usage: cardano-zkvms [setup]");
                eprintln!("  (no args)  Start the web server");
                eprintln!("  setup      One-time provisioning: build guest, keygen, agg keygen");
                std::process::exit(2);
            }
        }
    }

    // Resolve guest directory (the openvm guest crate)
    let guest_dir = std::env::var("OPENVM_GUEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // Default: assume we're run from web/backend/, guest is at ../../
            PathBuf::from("../../")
        })
        .canonicalize()
        .expect("Cannot resolve guest directory. Set OPENVM_GUEST_DIR env var.");

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);

    info!("OpenVM Web Backend starting");
    info!("  Guest dir:       {}", guest_dir.display());
    info!("  Port:            {}", port);

    let workspace_root = guest_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .expect(
            "Guest dir must be 3 levels deep from workspace root (e.g. crates/zkvms/openvm)",
        )
        .to_path_buf();

    // Resolve all paths
    let target_dir = workspace_root.join("target");
    let vmexe_path = target_dir.join("openvm/release/openvm-guest.vmexe");
    let pk_path = target_dir.join("openvm/app.pk");
    let config_path = guest_dir.join("openvm.toml");
    let openvm_home = openvm_home();
    let agg_pk_path = openvm_home.join("agg_stark.pk");

    info!("  Workspace root:  {}", workspace_root.display());
    info!("  Target dir:      {}", target_dir.display());
    info!("  OpenVM home:     {}", openvm_home.display());

    // Pre-flight: check for critical files
    let agg_vk_path = openvm_home.join("agg_stark.vk");
    let checks: &[(&str, &std::path::Path)] = &[
        ("Guest vmexe", &vmexe_path),
        ("Proving key", &pk_path),
        ("OpenVM config", &config_path),
        ("Agg STARK PK", &agg_pk_path),
        ("Agg STARK VK", &agg_vk_path),
    ];
    for (label, path) in checks {
        if path.exists() {
            info!("  {:15}  found", label);
        } else {
            tracing::warn!("  {:15}  NOT FOUND at {}", label, path.display());
        }
    }

    // Load all OpenVM artifacts at startup
    info!("Loading OpenVM artifacts...");
    let config = openvm_prover::load_config(&config_path)
        .expect("Failed to load openvm.toml config");
    let exe = openvm_prover::load_exe(&vmexe_path)
        .expect("Failed to load guest vmexe");
    let app_pk = openvm_prover::load_app_pk(&pk_path)
        .expect("Failed to load app proving key");
    let agg_pk = openvm_prover::load_agg_pk(&agg_pk_path)
        .expect("Failed to load aggregation proving key");
    info!("All artifacts loaded.");

    let state = web::Data::new(AppState {
        config,
        exe,
        app_pk,
        agg_pk,
        openvm_home,
    });

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
            // Serve agg_stark.vk from ~/.openvm/ (generated by `cardano-zkvms setup`)
            .route("/data/agg_stark.vk", web::get().to(serve_agg_stark_vk))
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}
