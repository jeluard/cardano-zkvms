use alloc::{string::String, vec};

use embedded_graphics::{pixelcolor::Rgb565, prelude::*};
use mousefood::{ColorTheme, EmbeddedBackend, EmbeddedBackendConfig};
use openvm_mcu_device_app::{ProofProbe, ProofStatus};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};

pub fn render_with_ratatui<D, F>(display: &mut D, draw: F) -> Result<(), ()>
where
    D: DrawTarget<Color = Rgb565> + Dimensions + 'static,
    F: FnOnce(&mut Frame),
{
    let config = EmbeddedBackendConfig {
        color_theme: ColorTheme {
            background: rgb(2, 4, 6),
            foreground: rgb(240, 244, 242),
            black: rgb(2, 4, 6),
            white: rgb(240, 244, 242),
            red: rgb(235, 94, 94),
            green: rgb(80, 220, 145),
            yellow: rgb(235, 205, 95),
            blue: rgb(93, 145, 245),
            magenta: rgb(210, 130, 245),
            cyan: rgb(80, 205, 220),
            ..ColorTheme::ansi()
        },
        ..Default::default()
    };
    let backend = EmbeddedBackend::new(display, config);
    let mut terminal = Terminal::new(backend).map_err(|_| ())?;
    terminal.draw(draw).map_err(|_| ())?;
    terminal.flush().map_err(|_| ())
}

pub fn draw_probe_frame(frame: &mut Frame, probe: &ProofProbe, board_label: &str, timing: &str) {
    let status_color = match probe.status {
        ProofStatus::Verified => Color::Green,
        ProofStatus::Rejected => Color::Red,
        ProofStatus::CryptoBackendUnavailable => Color::Yellow,
    };
    let [header, verifier, proof, host, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(5),
        Constraint::Length(10),
        Constraint::Length(5),
        Constraint::Min(1),
    ])
    .areas(frame.area());

    frame.render_widget(header_widget("Halo2/KZG", status_color), header);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "MCU verifier",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                probe.status.label(),
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(probe.status.detail()),
        ])
        .block(Block::bordered().border_style(status_color)),
        verifier,
    );

    frame.render_widget(
        Paragraph::new(vec![
            kv_line("kind", probe.proof_kind, Color::Yellow),
            kv_line("sha", short_text(probe.host.proof_sha.as_ref(), 30), Color::White),
            kv_line(
                "public",
                short_text(probe.host.public_values.as_ref(), 26),
                Color::White,
            ),
            kv_line_owned("bytes", decimal_alloc(probe.proof_data_len), Color::White),
            kv_line("time", short_text(timing, 28), status_color),
        ])
        .block(Block::bordered().title("Proof"))
        .wrap(Wrap { trim: true }),
        proof,
    );

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("Host pack", Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled(
                probe.host.status.as_ref(),
                Style::default().fg(Color::Yellow),
            )),
        ])
        .block(Block::bordered()),
        host,
    );

    frame.render_widget(
        Paragraph::new(board_label).style(Style::default().fg(Color::DarkGray)),
        footer,
    );
}

pub fn draw_ble_status_frame(
    frame: &mut Frame,
    status: &str,
    detail: &str,
    board_label: &str,
    proof_label: &str,
) {
    let status_color = status_color(status);
    let [header, ble, progress, proof, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(7),
        Constraint::Length(4),
        Constraint::Length(5),
        Constraint::Min(1),
    ])
    .areas(frame.area());

    frame.render_widget(header_widget("MCU Ratatui", status_color), header);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("BLE", Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled(
                status,
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(detail),
        ])
        .block(Block::bordered().border_style(status_color))
        .wrap(Wrap { trim: true }),
        ble,
    );

    frame.render_widget(
        Paragraph::new(vec![phase_bar(status), phase_legend(status)])
            .block(Block::bordered().title("Timing")),
        progress,
    );

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("PROOF", Style::default().fg(Color::DarkGray))),
            Line::from(proof_label),
            Line::from("BLE upload + KZG verify"),
        ])
        .block(Block::bordered()),
        proof,
    );

    frame.render_widget(
        Paragraph::new(board_label).style(Style::default().fg(Color::DarkGray)),
        footer,
    );
}

fn header_widget(label: &'static str, color: Color) -> Paragraph<'static> {
    Paragraph::new(Line::from(vec![
        Span::styled(
            "OpenVM ",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            label,
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
    ]))
    .block(Block::new().borders(Borders::BOTTOM).border_style(color))
}

fn kv_line<'a>(label: &'static str, value: &'a str, value_color: Color) -> Line<'a> {
    Line::from(vec![
        Span::styled(label, Style::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::styled(value, Style::default().fg(value_color)),
    ])
}

fn kv_line_owned(label: &'static str, value: String, value_color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(label, Style::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::styled(value, Style::default().fg(value_color)),
    ])
}

fn status_color(status: &str) -> Color {
    if status.starts_with("verified") {
        Color::Green
    } else if status.starts_with("error") || status.starts_with("rejected") {
        Color::Red
    } else if status.contains("verifying") || status.contains("verify") {
        Color::Yellow
    } else {
        Color::Cyan
    }
}

fn phase_bar(status: &str) -> Line<'static> {
    let active = phase_index(status);
    Line::from(vec![
        phase_span(" WAIT ", Color::Blue, active, 0),
        Span::raw(" "),
        phase_span(" RX ", Color::Cyan, active, 1),
        Span::raw(" "),
        phase_span(" VERIFY ", Color::Yellow, active, 2),
        Span::raw(" "),
        phase_span(" DONE ", status_color(status), active, 3),
    ])
}

fn phase_legend(status: &str) -> Line<'static> {
    let label = if status.starts_with("ready") {
        "waiting"
    } else if status.starts_with("receiving") {
        "uploading"
    } else if status.starts_with("received") {
        "upload done"
    } else if status.contains("verifying") || status.contains("verify") {
        "verifying"
    } else if status.starts_with("verified") {
        "accepted"
    } else if status.starts_with("rejected") {
        "rejected"
    } else if status.starts_with("error") {
        "error"
    } else {
        "active"
    };
    Line::from(vec![
        Span::styled("phase ", Style::default().fg(Color::DarkGray)),
        Span::styled(label, Style::default().fg(status_color(status))),
    ])
}

fn phase_index(status: &str) -> usize {
    if status.starts_with("ready") {
        0
    } else if status.starts_with("receiving") || status.starts_with("received") {
        1
    } else if status.contains("verifying") || status.contains("verify") {
        2
    } else {
        3
    }
}

fn phase_span(label: &'static str, color: Color, active: usize, index: usize) -> Span<'static> {
    let mut style = Style::default();
    if index < active {
        style = style.fg(Color::Black).bg(color);
    } else if index == active {
        style = style.fg(Color::Black).bg(color).add_modifier(Modifier::BOLD);
    } else {
        style = style.fg(Color::DarkGray);
    }
    Span::styled(label, style)
}

fn decimal_alloc(mut value: usize) -> String {
    let mut buffer = [0_u8; 20];
    let mut index = buffer.len();
    if value == 0 {
        index -= 1;
        buffer[index] = b'0';
    }
    while value > 0 {
        index -= 1;
        buffer[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    core::str::from_utf8(&buffer[index..]).unwrap_or("?").into()
}

fn short_text(value: &str, max_bytes: usize) -> &str {
    value.get(..max_bytes).unwrap_or(value)
}

const fn rgb(red: u8, green: u8, blue: u8) -> embedded_graphics::pixelcolor::Rgb888 {
    embedded_graphics::pixelcolor::Rgb888::new(red, green, blue)
}