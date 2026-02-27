//! Real-time TUI dashboard using ratatui + crossterm.
//!
//! # Layout
//! ```text
//! ┌─ Title bar: server IP, ports, uptime ─────────────────────────┐
//! │  Connected peers table                                         │
//! │    VPN IP  │ Endpoint │ ↓ Download │ ↑ Upload │ Total │ Limit │
//! ├─ Totals bar ───────────────────────────────────────────────────┤
//! │  Log tail (last 6 entries)                                     │
//! └─ Hint line ────────────────────────────────────────────────────┘
//! ```
//!
//! # Safety
//! The dashboard is **only started when `stdout` is a real TTY** (checked in
//! `main.rs`).  This prevents `crossterm::enable_raw_mode()` from hijacking
//! SSH sessions or `systemd` journal output, which was the original cause of
//! the "server hangs at startup" bug.
//!
//! # Keys
//! * `q` / `Esc` — exit server gracefully

use std::{sync::atomic::Ordering, time::Duration};

use anyhow::Result;
use crossterm::{
    event::{Event, EventStream, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Terminal,
};
use tokio::time;

use crate::state::Shared;

// ── Speed snapshot ────────────────────────────────────────────────────────────

/// Per-peer byte counters captured at the previous tick.
///
/// Stored in a local `HashMap` by the dashboard task and used to compute the
/// per-second speed by subtracting the previous sample from the current one.
struct SpeedSnap {
    bytes_in: u64,
    bytes_out: u64,
}

// ── Dashboard entry point ─────────────────────────────────────────────────────

/// Run the interactive TUI dashboard until the user presses `q` or `Esc`.
///
/// Enters crossterm's alternate screen so the dashboard doesn't pollute the
/// terminal history.  The terminal is restored cleanly even on error thanks
/// to the explicit cleanup at the end.
///
/// # Tick rate
/// The display refreshes every second.  Speed values are computed by comparing
/// the current byte counters to the previous-second snapshot.
///
/// # Note
/// This function must only be called when `stdout` is a real TTY.  In
/// non-TTY environments (SSH, CI, systemd) `main.rs` skips this and falls
/// back to a plain `Ctrl-C` wait.
pub async fn run_dashboard(state: Shared) -> Result<()> {
    // Enter raw mode and alternate screen
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut events = EventStream::new();
    let mut ticker = time::interval(Duration::from_secs(1));

    // Per-peer previous-second byte counters for speed computation
    let mut prev: std::collections::HashMap<String, SpeedSnap> = Default::default();

    loop {
        tokio::select! {
            // ── Tick: update speeds and redraw ────────────────────────────────
            _ = ticker.tick() => {
                // Compute per-peer instantaneous speed (bytes/s over last second)
                for entry in state.peers.iter() {
                    let p = entry.value();
                    let key = p.vpn_ip.to_string();
                    let cur_in  = p.bytes_in.load(Ordering::Relaxed);
                    let cur_out = p.bytes_out.load(Ordering::Relaxed);
                    if let Some(snap) = prev.get(&key) {
                        // Delta over the last second = instantaneous bytes/s
                        let speed_in  = cur_in.saturating_sub(snap.bytes_in);
                        let speed_out = cur_out.saturating_sub(snap.bytes_out);
                        p.speed_in_bps.store(speed_in, Ordering::Relaxed);
                        p.speed_out_bps.store(speed_out, Ordering::Relaxed);
                    }
                    prev.insert(key, SpeedSnap { bytes_in: cur_in, bytes_out: cur_out });
                }
                // Redraw the full frame
                // block_in_place lets the synchronous draw() call blocking_read()
                // on tokio RwLocks without panicking inside the async runtime.
                tokio::task::block_in_place(|| terminal.draw(|f| draw(f, &state)))?;
            }

            // ── Keyboard events ───────────────────────────────────────────────
            maybe_ev = events.next() => {
                if let Some(Ok(Event::Key(key))) = maybe_ev {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => break,
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    // Restore terminal to its original state
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Render the full dashboard frame.
///
/// Called once per second by the tick handler.  All rendering is synchronous
/// (uses `blocking_read` for locks) because ratatui's `draw` closure cannot
/// be async.
fn draw(f: &mut ratatui::Frame, state: &Shared) {
    let area = f.area();

    // Divide the terminal vertically into five sections
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // title bar
            Constraint::Min(6),    // peer table (expands to fill)
            Constraint::Length(3), // totals bar
            Constraint::Length(8), // log tail (6 lines + border)
            Constraint::Length(1), // hint line
        ])
        .split(area);

    // ── Title bar ─────────────────────────────────────────────────────────────
    let pub_ip  = state.public_ip.blocking_read().clone();
    let local_ip = state.local_ip.blocking_read().clone();
    let uptime  = fmt_duration(state.uptime_secs());

    let title_text = Line::from(vec![
        Span::styled(" LOWKEY VPN ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled("Public: ",  Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{pub_ip}"),   Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled("Local: ",   Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{local_ip}")),
        Span::raw("  "),
        Span::styled("UDP: ",     Style::default().fg(Color::DarkGray)),
        Span::raw(format!(":{}", state.udp_port)),
        Span::raw("  "),
        Span::styled("Proxy: ",   Style::default().fg(Color::DarkGray)),
        Span::raw(format!(":{}", state.proxy_port)),
        Span::raw("  "),
        Span::styled("Uptime: ",  Style::default().fg(Color::DarkGray)),
        Span::styled(uptime, Style::default().fg(Color::Green)),
    ]);
    let title = Paragraph::new(title_text)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)));
    f.render_widget(title, chunks[0]);

    // ── Peers table ───────────────────────────────────────────────────────────
    let peer_count = state.peers.len();

    let header_cells = ["VPN IP", "Endpoint", "↓ Download", "↑ Upload", "Total ↓", "Total ↑", "Limit"]
        .iter()
        .map(|h| Cell::from(*h).style(
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        ));
    let header = Row::new(header_cells)
        .style(Style::default().bg(Color::DarkGray))
        .height(1);

    let rows: Vec<Row> = state.peers.iter().map(|entry| {
        let p = entry.value();

        // Read endpoint (non-blocking — dashboard doesn't await)
        let ep = p.endpoint.blocking_read()
            .map(|e| e.to_string())
            .unwrap_or_else(|| "connecting…".into());

        let speed_in  = p.speed_in_bps.load(Ordering::Relaxed);
        let speed_out = p.speed_out_bps.load(Ordering::Relaxed);
        let lim       = p.limit_bps.load(Ordering::Relaxed);

        let limit_str = if lim == 0 { "∞".into() } else { fmt_speed(lim) };

        Row::new(vec![
            Cell::from(p.vpn_ip.to_string()).style(Style::default().fg(Color::Cyan)),
            Cell::from(ep).style(Style::default().fg(Color::White)),
            Cell::from(fmt_speed(speed_in)).style(speed_color(speed_in)),
            Cell::from(fmt_speed(speed_out)).style(speed_color(speed_out)),
            Cell::from(fmt_bytes(p.bytes_in())).style(Style::default().fg(Color::DarkGray)),
            Cell::from(fmt_bytes(p.bytes_out())).style(Style::default().fg(Color::DarkGray)),
            Cell::from(limit_str).style(Style::default().fg(Color::Magenta)),
        ])
        .height(1)
    }).collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(12), // VPN IP
            Constraint::Length(22), // Endpoint
            Constraint::Length(12), // ↓ speed
            Constraint::Length(12), // ↑ speed
            Constraint::Length(10), // total ↓
            Constraint::Length(10), // total ↑
            Constraint::Length(8),  // limit
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue))
            .title(format!(" Connected Peers ({peer_count}) ")),
    )
    .row_highlight_style(Style::default().add_modifier(Modifier::BOLD));
    f.render_widget(table, chunks[1]);

    // ── Totals bar ────────────────────────────────────────────────────────────
    let total_in  = state.total_bytes_in.load(Ordering::Relaxed);
    let total_out = state.total_bytes_out.load(Ordering::Relaxed);

    // Sum instantaneous speeds across all connected peers
    let sum_speed_in:  u64 = state.peers.iter().map(|e| e.speed_in_bps.load(Ordering::Relaxed)).sum();
    let sum_speed_out: u64 = state.peers.iter().map(|e| e.speed_out_bps.load(Ordering::Relaxed)).sum();

    let totals = Paragraph::new(Line::from(vec![
        Span::styled(" Total ↓: ",    Style::default().fg(Color::DarkGray)),
        Span::styled(fmt_speed(sum_speed_in),  Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled("Total ↑: ",    Style::default().fg(Color::DarkGray)),
        Span::styled(fmt_speed(sum_speed_out), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled("All-time ↓: ", Style::default().fg(Color::DarkGray)),
        Span::raw(fmt_bytes(total_in)),
        Span::raw("  "),
        Span::styled("All-time ↑: ", Style::default().fg(Color::DarkGray)),
        Span::raw(fmt_bytes(total_out)),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Green)));
    f.render_widget(totals, chunks[2]);

    // ── Log tail ──────────────────────────────────────────────────────────────
    // Show the last 6 lines from the in-memory log ring-buffer
    let log_lines: Vec<Line> = {
        let buf = state.logs.blocking_lock();
        let skip = buf.len().saturating_sub(6);
        buf.iter()
            .skip(skip)
            .map(|msg| Line::from(Span::styled(
                msg.clone(),
                Style::default().fg(Color::DarkGray),
            )))
            .collect()
    };
    let log_widget = Paragraph::new(log_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Logs "),
        );
    f.render_widget(log_widget, chunks[3]);

    // ── Hint line ─────────────────────────────────────────────────────────────
    let hints = Paragraph::new(
        "  [q] Quit  •  API: PUT /api/peers/:ip/limit  {\"limit_mbps\":10}  •  DELETE /api/peers/:ip"
    )
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hints, chunks[4]);
}

// ── Formatting helpers ────────────────────────────────────────────────────────

/// Format bytes/s as a human-readable speed string (B/s, KB/s, MB/s).
fn fmt_speed(bps: u64) -> String {
    if bps == 0 {
        "0 B/s".into()
    } else if bps < 1024 {
        format!("{bps} B/s")
    } else if bps < 1024 * 1024 {
        format!("{:.1} KB/s", bps as f64 / 1024.0)
    } else {
        format!("{:.2} MB/s", bps as f64 / 1_048_576.0)
    }
}

/// Format a byte count as a human-readable size string (B, KB, MB, GB).
fn fmt_bytes(b: u64) -> String {
    if b < 1024 {
        format!("{b} B")
    } else if b < 1024 * 1024 {
        format!("{:.1} KB", b as f64 / 1024.0)
    } else if b < 1024 * 1024 * 1024 {
        format!("{:.2} MB", b as f64 / 1_048_576.0)
    } else {
        format!("{:.2} GB", b as f64 / 1_073_741_824.0)
    }
}

/// Format a duration given in seconds as `HH:MM:SS`.
fn fmt_duration(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

/// Choose a terminal color based on speed magnitude.
///
/// * Dark gray  — zero (idle)
/// * Green      — below 1 MB/s
/// * Bright green + bold — 1 MB/s and above
fn speed_color(bps: u64) -> Style {
    if bps == 0 {
        Style::default().fg(Color::DarkGray)
    } else if bps < 1_000_000 {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
    }
}
