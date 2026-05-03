use openvm_mcu_device_app::ProofProbe;

use crate::tufty_display::TuftyDisplay;

#[path = "../../common/ratatui_ui.rs"]
mod ratatui_ui;

const BOARD_LABEL: &str = "Tufty RP2350";
const PROOF_LABEL: &str = "native Halo2/KZG";

pub fn render_status(status: &str, detail: &str, _proof_sha: &str) -> Result<(), ()> {
    let mut display = TuftyDisplay::new();
    ratatui_ui::render_with_ratatui(&mut display, |frame| {
        ratatui_ui::draw_ble_status_frame(frame, status, detail, BOARD_LABEL, PROOF_LABEL)
    })
}

pub fn render_probe(probe: &ProofProbe, timing: &str) -> Result<(), ()> {
    let mut display = TuftyDisplay::new();
    ratatui_ui::render_with_ratatui(&mut display, |frame| {
        ratatui_ui::draw_probe_frame(frame, probe, BOARD_LABEL, timing)
    })
}