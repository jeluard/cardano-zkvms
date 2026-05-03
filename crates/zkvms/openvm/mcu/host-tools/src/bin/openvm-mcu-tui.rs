use std::{fs, io, path::PathBuf, time::Duration};

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use openvm_mcu_verifier_core::{
    decode_frame, decode_message, FrameType, OpenVmEvmHalo2Verifier, ProofEnvelope, Verifier,
    VerifierKey, FRAME_MAGIC,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
    Frame, Terminal,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Parser, Clone)]
#[command(
    author,
    version,
    about = "Ratatui dashboard for OpenVM MCU proof artifacts"
)]
struct Cli {
    #[arg(long, help = "Packed verifier key envelope")]
    key: Option<PathBuf>,
    #[arg(long, help = "Packed proof envelope")]
    proof: Option<PathBuf>,
    #[arg(long, help = "JSON response from /api/prove/mcu-halo2")]
    backend_response: Option<PathBuf>,
    #[arg(long, default_value_t = 250)]
    tick_millis: u64,
}

#[derive(Default)]
struct Dashboard {
    artifact_source: String,
    key_bytes: Option<usize>,
    proof_bytes: Option<usize>,
    proof_sha256: Option<String>,
    proof_kind: Option<String>,
    openvm_version: Option<String>,
    public_values_len: Option<usize>,
    proof_data_len: Option<usize>,
    local_verdict: String,
    ble_service_uuid: Option<String>,
    ble_control_uuid: Option<String>,
    ble_data_uuid: Option<String>,
    ble_status_uuid: Option<String>,
    ble_chunk_bytes: Option<usize>,
    logs: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut terminal = init_terminal()?;
    let result = run_app(&mut terminal, &cli);
    restore_terminal(&mut terminal)?;
    result
}

fn init_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, cli: &Cli) -> Result<()> {
    let mut dashboard = load_dashboard(cli);
    loop {
        terminal.draw(|frame| render(frame, &dashboard))?;
        if event::poll(Duration::from_millis(cli.tick_millis))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('r') => dashboard = load_dashboard(cli),
                    _ => {}
                },
                _ => {}
            }
        }
    }
}

fn load_dashboard(cli: &Cli) -> Dashboard {
    let mut dashboard = Dashboard {
        local_verdict: "not run".to_owned(),
        ..Dashboard::default()
    };

    match load_artifacts(cli) {
        Ok(Some(artifacts)) => apply_artifacts(&mut dashboard, artifacts),
        Ok(None) => {
            dashboard.artifact_source = "none".to_owned();
            dashboard
                .logs
                .push("Pass --backend-response, or --key and --proof.".to_owned());
        }
        Err(error) => {
            dashboard.artifact_source = "error".to_owned();
            dashboard.local_verdict = "error".to_owned();
            dashboard.logs.push(format!("load error: {error:#}"));
        }
    }

    dashboard
}

struct Artifacts {
    source: String,
    key_bytes: Vec<u8>,
    proof_bytes: Vec<u8>,
    proof_sha256: Option<String>,
    ble: Option<BleInfo>,
}

struct BleInfo {
    service_uuid: Option<String>,
    control_uuid: Option<String>,
    data_uuid: Option<String>,
    status_uuid: Option<String>,
    chunk_bytes: Option<usize>,
}

fn load_artifacts(cli: &Cli) -> Result<Option<Artifacts>> {
    if let Some(path) = &cli.backend_response {
        return load_backend_response(path).map(Some);
    }

    match (&cli.key, &cli.proof) {
        (Some(key_path), Some(proof_path)) => Ok(Some(Artifacts {
            source: format!("{} + {}", key_path.display(), proof_path.display()),
            key_bytes: fs::read(key_path)
                .with_context(|| format!("read {}", key_path.display()))?,
            proof_bytes: fs::read(proof_path)
                .with_context(|| format!("read {}", proof_path.display()))?,
            proof_sha256: None,
            ble: None,
        })),
        (None, None) => Ok(None),
        _ => bail!("--key and --proof must be provided together"),
    }
}

fn load_backend_response(path: &PathBuf) -> Result<Artifacts> {
    let json: Value = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("read {}", path.display()))?,
    )
    .with_context(|| format!("parse {}", path.display()))?;

    let verifier_key_b64 = required_json_str(&json, "verifier_key_b64")?;
    let proof_envelope_b64 = required_json_str(&json, "proof_envelope_b64")?;
    let ble = json.get("ble");

    Ok(Artifacts {
        source: path.display().to_string(),
        key_bytes: BASE64.decode(verifier_key_b64)?,
        proof_bytes: BASE64.decode(proof_envelope_b64)?,
        proof_sha256: json
            .get("proof_sha256")
            .and_then(Value::as_str)
            .map(str::to_owned),
        ble: Some(BleInfo {
            service_uuid: ble.and_then(|value| json_string(value, "service_uuid")),
            control_uuid: ble.and_then(|value| json_string(value, "control_uuid")),
            data_uuid: ble.and_then(|value| json_string(value, "data_uuid")),
            status_uuid: ble.and_then(|value| json_string(value, "status_uuid")),
            chunk_bytes: ble
                .and_then(|value| value.get("chunk_bytes"))
                .and_then(Value::as_u64)
                .map(|value| value as usize),
        }),
    })
}

fn apply_artifacts(dashboard: &mut Dashboard, artifacts: Artifacts) {
    dashboard.artifact_source = artifacts.source;
    dashboard.key_bytes = Some(artifacts.key_bytes.len());
    dashboard.proof_bytes = Some(artifacts.proof_bytes.len());
    dashboard.proof_sha256 = Some(
        artifacts
            .proof_sha256
            .unwrap_or_else(|| hex::encode(Sha256::digest(&artifacts.proof_bytes))),
    );

    if let Some(ble) = artifacts.ble {
        dashboard.ble_service_uuid = ble.service_uuid;
        dashboard.ble_control_uuid = ble.control_uuid;
        dashboard.ble_data_uuid = ble.data_uuid;
        dashboard.ble_status_uuid = ble.status_uuid;
        dashboard.ble_chunk_bytes = ble.chunk_bytes;
    }

    let key: VerifierKey = match decode_artifact(&artifacts.key_bytes, FrameType::VerifierKey) {
        Ok(key) => key,
        Err(error) => {
            dashboard.local_verdict = "decode error".to_owned();
            dashboard.logs.push(format!("key decode failed: {error:#}"));
            return;
        }
    };
    let proof: ProofEnvelope =
        match decode_artifact(&artifacts.proof_bytes, FrameType::ProofEnvelope) {
            Ok(proof) => proof,
            Err(error) => {
                dashboard.local_verdict = "decode error".to_owned();
                dashboard
                    .logs
                    .push(format!("proof decode failed: {error:#}"));
                return;
            }
        };

    dashboard.proof_kind = Some(format!("{:?}", proof.proof_kind));
    dashboard.openvm_version = Some(proof.openvm_version.clone());
    dashboard.public_values_len = Some(proof.user_public_values.len());
    dashboard.proof_data_len = Some(proof.proof_data.len());

    let mut verifier = OpenVmEvmHalo2Verifier::default();
    match verifier.verify(&key, &proof) {
        Ok(report) if report.verified => {
            dashboard.local_verdict = "verified".to_owned();
            dashboard
                .logs
                .push("native verifier accepted the proof".to_owned());
        }
        Ok(_) => {
            dashboard.local_verdict = "rejected".to_owned();
            dashboard
                .logs
                .push("native verifier rejected the proof".to_owned());
        }
        Err(error) => {
            dashboard.local_verdict = "error".to_owned();
            dashboard
                .logs
                .push(format!("native verifier error: {error:#}"));
        }
    }
}

fn decode_artifact<'bytes, T>(bytes: &'bytes [u8], expected_frame_type: FrameType) -> Result<T>
where
    T: serde::Deserialize<'bytes>,
{
    if bytes.starts_with(&FRAME_MAGIC) {
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

fn render(frame: &mut Frame<'_>, dashboard: &Dashboard) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "OpenVM MCU Dashboard",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::raw("q quit | r reload"),
    ]))
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, chunks[0]);

    frame.render_widget(artifact_table(dashboard), chunks[1]);
    frame.render_widget(ble_table(dashboard), chunks[2]);

    let log_lines: Vec<Line<'_>> = dashboard
        .logs
        .iter()
        .rev()
        .take(8)
        .rev()
        .map(|message| Line::from(message.as_str()))
        .collect();
    let logs = Paragraph::new(log_lines)
        .block(Block::default().title("Log").borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    frame.render_widget(logs, chunks[3]);

    let footer = Paragraph::new("This dashboard only inspects/verifies artifacts locally; browser Web Bluetooth or a sender still performs BLE transfer.")
        .block(Block::default().borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    frame.render_widget(footer, chunks[4]);
}

fn artifact_table(dashboard: &Dashboard) -> Table<'static> {
    let verdict_style = match dashboard.local_verdict.as_str() {
        "verified" => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        "rejected" | "error" | "decode error" => Style::default().fg(Color::Red),
        _ => Style::default().fg(Color::Yellow),
    };
    let rows = vec![
        row("source", &dashboard.artifact_source),
        row("local verdict", &dashboard.local_verdict).style(verdict_style),
        row("proof kind", &display_option(&dashboard.proof_kind)),
        row("OpenVM version", &display_option(&dashboard.openvm_version)),
        row("key envelope", &display_bytes(dashboard.key_bytes)),
        row("proof envelope", &display_bytes(dashboard.proof_bytes)),
        row("proof data", &display_bytes(dashboard.proof_data_len)),
        row("public values", &display_bytes(dashboard.public_values_len)),
        row(
            "proof sha256",
            dashboard.proof_sha256.as_deref().unwrap_or("-"),
        ),
    ];

    Table::new(rows, [Constraint::Length(18), Constraint::Min(20)])
        .block(Block::default().title("Artifacts").borders(Borders::ALL))
        .column_spacing(2)
}

fn ble_table(dashboard: &Dashboard) -> Table<'static> {
    let rows = vec![
        row(
            "service",
            dashboard.ble_service_uuid.as_deref().unwrap_or("-"),
        ),
        row(
            "control",
            dashboard.ble_control_uuid.as_deref().unwrap_or("-"),
        ),
        row("data", dashboard.ble_data_uuid.as_deref().unwrap_or("-")),
        row(
            "status",
            dashboard.ble_status_uuid.as_deref().unwrap_or("-"),
        ),
        row(
            "chunk bytes",
            &dashboard
                .ble_chunk_bytes
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_owned()),
        ),
    ];

    Table::new(rows, [Constraint::Length(18), Constraint::Min(20)])
        .block(Block::default().title("BLE").borders(Borders::ALL))
        .column_spacing(2)
}

fn row(label: impl Into<String>, value: impl Into<String>) -> Row<'static> {
    Row::new(vec![
        Cell::from(label.into()).style(Style::default().fg(Color::Blue)),
        Cell::from(value.into()),
    ])
}

fn display_option(value: &Option<String>) -> String {
    value.as_deref().unwrap_or("-").to_owned()
}

fn display_bytes(value: Option<usize>) -> String {
    value
        .map(|bytes| format!("{bytes} bytes"))
        .unwrap_or_else(|| "-".to_owned())
}

fn required_json_str<'json>(value: &'json Value, field: &str) -> Result<&'json str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .with_context(|| format!("missing string field {field}"))
}

fn json_string(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_owned)
}
