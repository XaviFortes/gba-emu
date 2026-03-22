use std::fs;
use std::path::{Path, PathBuf};

use font8x8::{BASIC_FONTS, UnicodeFonts};
use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};

#[derive(Debug, Clone, Copy)]
pub enum LauncherAudioOutput {
    Default,
    Muted,
}

#[derive(Debug, Clone)]
pub struct LauncherSelection {
    pub rom_path: String,
    pub bios_path: Option<String>,
    pub scale: Scale,
    pub audio_output: LauncherAudioOutput,
    pub master_volume: f32,
}

#[derive(Debug)]
struct LauncherState {
    roms: Vec<PathBuf>,
    selected_rom: usize,
    selected_setting: usize,
    scale: u32,
    use_bios: bool,
    bios_candidate: Option<PathBuf>,
    audio_output: LauncherAudioOutput,
    master_volume_percent: u8,
    audio_backend_info: String,
}

const UI_WIDTH: usize = 1280;
const UI_HEIGHT: usize = 720;

pub fn run_launcher(
    roms_dir: Option<&str>,
    bios_arg: Option<&str>,
    default_scale: u32,
    audio_backend_info: &str,
) -> Result<Option<LauncherSelection>, String> {
    let mut window = Window::new(
        "GBA Emulator - Launcher",
        UI_WIDTH,
        UI_HEIGHT,
        WindowOptions {
            resize: false,
            scale: Scale::X1,
            ..WindowOptions::default()
        },
    )
    .map_err(|err| format!("Failed to create launcher window: {err}"))?;

    window.set_target_fps(60);

    let rom_root = roms_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let mut state = LauncherState {
        roms: scan_roms(&rom_root),
        selected_rom: 0,
        selected_setting: 0,
        scale: default_scale.clamp(1, 6),
        use_bios: true,
        bios_candidate: resolve_bios_candidate(bios_arg),
        audio_output: LauncherAudioOutput::Default,
        master_volume_percent: 80,
        audio_backend_info: audio_backend_info.to_string(),
    };

    if state.bios_candidate.is_none() {
        state.use_bios = false;
    }

    let mut fb = vec![0u32; UI_WIDTH * UI_HEIGHT];

    while window.is_open() && !window.is_key_down(Key::Escape) {
        handle_input(&mut window, &mut state, &rom_root);

        if window.is_key_pressed(Key::Enter, KeyRepeat::No) {
            if let Some(selection) = finalize_selection(&state) {
                return Ok(Some(selection));
            }
        }

        draw_launcher(&mut fb, &state, &rom_root);

        if let Err(err) = window.update_with_buffer(&fb, UI_WIDTH, UI_HEIGHT) {
            return Err(format!("Launcher drawing failed: {err}"));
        }
    }

    Ok(None)
}

fn finalize_selection(state: &LauncherState) -> Option<LauncherSelection> {
    let rom_path = state.roms.get(state.selected_rom)?.to_string_lossy().to_string();

    Some(LauncherSelection {
        rom_path,
        bios_path: if state.use_bios {
            state
                .bios_candidate
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
        } else {
            None
        },
        scale: scale_from_u32(state.scale),
        audio_output: state.audio_output,
        master_volume: (state.master_volume_percent as f32 / 100.0).clamp(0.0, 1.0),
    })
}

fn handle_input(window: &mut Window, state: &mut LauncherState, rom_root: &Path) {
    if window.is_key_pressed(Key::Left, KeyRepeat::Yes) {
        if state.selected_rom > 0 {
            state.selected_rom -= 1;
        }
    }

    if window.is_key_pressed(Key::Right, KeyRepeat::Yes)
        && state.selected_rom + 1 < state.roms.len()
    {
        state.selected_rom += 1;
    }

    if window.is_key_pressed(Key::Up, KeyRepeat::Yes) {
        state.selected_setting = state.selected_setting.saturating_sub(1);
    }

    if window.is_key_pressed(Key::Down, KeyRepeat::Yes) {
        state.selected_setting = (state.selected_setting + 1).min(3);
    }

    if window.is_key_pressed(Key::A, KeyRepeat::No) {
        adjust_setting(state, false);
    }

    if window.is_key_pressed(Key::D, KeyRepeat::No) {
        adjust_setting(state, true);
    }

    if window.is_key_pressed(Key::R, KeyRepeat::No) {
        state.roms = scan_roms(rom_root);
        if state.selected_rom >= state.roms.len() {
            state.selected_rom = 0;
        }
    }
}

fn adjust_setting(state: &mut LauncherState, increase: bool) {
    match state.selected_setting {
        0 => {
            if increase {
                state.scale = (state.scale + 1).min(6);
            } else {
                state.scale = state.scale.saturating_sub(1).max(1);
            }
        }
        1 => {
            if state.bios_candidate.is_some() {
                state.use_bios = !state.use_bios;
            }
        }
        _ => {
            if state.selected_setting == 2 {
                state.audio_output = match state.audio_output {
                    LauncherAudioOutput::Default => LauncherAudioOutput::Muted,
                    LauncherAudioOutput::Muted => LauncherAudioOutput::Default,
                }
            } else if increase {
                state.master_volume_percent = (state.master_volume_percent + 5).min(100);
            } else {
                state.master_volume_percent = state.master_volume_percent.saturating_sub(5);
            }
        }
    }
}

fn resolve_bios_candidate(bios_arg: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = bios_arg {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }

    let fallback = PathBuf::from("gba_bios.bin");
    if fallback.is_file() {
        return Some(fallback);
    }

    None
}

fn scan_roms(dir: &Path) -> Vec<PathBuf> {
    let mut roms = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return roms,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };

        if ext.eq_ignore_ascii_case("gba") {
            roms.push(path);
        }
    }

    roms.sort();
    roms
}

fn draw_launcher(fb: &mut [u32], state: &LauncherState, rom_root: &Path) {
    draw_background(fb);

    draw_text(fb, 48, 44, 3, 0xFFF6_F6F8, "GBA Library");
    draw_text(
        fb,
        48,
        84,
        1,
        0xFFB9_C3D1,
        "Nintendo Switch-inspired launcher | Left/Right: game | Up/Down: setting | A/D: change | Enter: launch",
    );

    draw_text(
        fb,
        48,
        112,
        1,
        0xFF9D_A7B6,
        &format!("ROM Directory: {}", rom_root.display()),
    );

    draw_game_carousel(fb, state);
    draw_settings_panel(fb, state);

    if state.roms.is_empty() {
        draw_rect(fb, 72, 220, 760, 96, 0xAA33_2B2B);
        draw_text(
            fb,
            98,
            258,
            2,
            0xFFFF_DDDD,
            "No .gba ROMs found in current directory.",
        );
    }
}

fn draw_game_carousel(fb: &mut [u32], state: &LauncherState) {
    let card_w = 232;
    let card_h = 332;
    let card_gap = 26;
    let x0 = 72;
    let y0 = 178;

    let start = state.selected_rom.saturating_sub(1);
    let end = (start + 4).min(state.roms.len());

    for (slot, idx) in (start..end).enumerate() {
        let x = x0 + slot as i32 * (card_w + card_gap);
        let is_selected = idx == state.selected_rom;

        let frame = if is_selected { 0xFF45_D6E8 } else { 0xFF4A_576B };
        let fill = if is_selected { 0xCC17_2435 } else { 0xAA12_1822 };
        draw_rect(fb, x, y0, card_w, card_h, fill);
        draw_outline(fb, x, y0, card_w, card_h, frame, 3);

        draw_rect(fb, x + 16, y0 + 18, card_w - 32, 190, 0xFF1D_2B40);
        draw_rect(fb, x + 24, y0 + 26, card_w - 48, 88, 0xFF2A_3F5E);
        draw_rect(fb, x + 24, y0 + 122, card_w - 48, 78, 0xFF374C_70);

        let name = state.roms[idx]
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown.gba");

        draw_wrapped_text(
            fb,
            x + 16,
            y0 + 228,
            card_w - 28,
            1,
            if is_selected { 0xFFEE_F7FF } else { 0xFFD3_DBE8 },
            name,
        );

        if is_selected {
            draw_text(fb, x + 16, y0 + card_h - 28, 1, 0xFF66_EAFF, "READY");
        }
    }
}

fn draw_settings_panel(fb: &mut [u32], state: &LauncherState) {
    let panel_x = 910;
    let panel_y = 178;
    let panel_w = 320;
    let panel_h = 420;

    draw_rect(fb, panel_x, panel_y, panel_w, panel_h, 0xB013_1A27);
    draw_outline(fb, panel_x, panel_y, panel_w, panel_h, 0xFF60_748F, 2);

    draw_text(fb, panel_x + 20, panel_y + 20, 2, 0xFFE4_E9F2, "Settings");

    let labels = [
        format!("Window Scale: {}x", state.scale),
        format!(
            "Use BIOS: {}",
            if state.use_bios {
                state
                    .bios_candidate
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or("ON")
            } else {
                "OFF"
            }
        ),
        format!(
            "Audio Output: {}",
            match state.audio_output {
                LauncherAudioOutput::Default => "Default",
                LauncherAudioOutput::Muted => "Muted",
            }
        ),
        format!("Master Volume: {}%", state.master_volume_percent),
    ];

    for (i, text) in labels.iter().enumerate() {
        let y = panel_y + 86 + i as i32 * 66;
        let selected = i == state.selected_setting;
        let row_bg = if selected { 0xFF2C_3A52 } else { 0xFF1B_2535 };
        let row_fg = if selected { 0xFFEE_F7FF } else { 0xFFD2_DBE8 };
        draw_rect(fb, panel_x + 18, y, panel_w - 36, 48, row_bg);
        draw_outline(
            fb,
            panel_x + 18,
            y,
            panel_w - 36,
            48,
            if selected { 0xFF6D_DFF0 } else { 0xFF455873 },
            1,
        );
        draw_text(fb, panel_x + 30, y + 16, 1, row_fg, text);
    }

    draw_text(fb, panel_x + 20, panel_y + 318, 1, 0xFF9F_AEC2, "Audio Backend");
    draw_wrapped_text(
        fb,
        panel_x + 20,
        panel_y + 338,
        panel_w - 40,
        1,
        0xFFC6_D2E4,
        &state.audio_backend_info,
    );

    draw_text(fb, panel_x + 20, panel_y + 384, 1, 0xFF9F_AEC2, "Controls");
    draw_text(fb, panel_x + 20, panel_y + 402, 1, 0xFFC6_D2E4, "Left/Right: Pick game");
    draw_text(fb, panel_x + 20, panel_y + 420, 1, 0xFFC6_D2E4, "Up/Down: Select setting");
    draw_text(fb, panel_x + 20, panel_y + 438, 1, 0xFFC6_D2E4, "A / D: Change value");
    draw_text(fb, panel_x + 20, panel_y + 456, 1, 0xFFC6_D2E4, "R: Rescan ROMs");
    draw_text(fb, panel_x + 20, panel_y + 474, 1, 0xFFC6_D2E4, "Enter: Launch game");
}

fn scale_from_u32(value: u32) -> Scale {
    match value {
        1 => Scale::X1,
        2 => Scale::X2,
        3 => Scale::X4,
        4 => Scale::X8,
        5 => Scale::X16,
        6 => Scale::X32,
        _ => Scale::X4,
    }
}

fn draw_background(fb: &mut [u32]) {
    for y in 0..UI_HEIGHT {
        let t = y as f32 / UI_HEIGHT as f32;
        let r = lerp(8, 30, t);
        let g = lerp(14, 34, t);
        let b = lerp(28, 64, t);
        let color = pack_rgb(r, g, b);
        let row = y * UI_WIDTH;
        for x in 0..UI_WIDTH {
            fb[row + x] = color;
        }
    }

    draw_rect(fb, -180, -120, 720, 420, 0x4422_4C7A);
    draw_rect(fb, 780, 420, 620, 360, 0x4418_5C7C);
}

fn draw_wrapped_text(
    fb: &mut [u32],
    x: i32,
    y: i32,
    max_width: i32,
    scale: i32,
    color: u32,
    text: &str,
) {
    let max_chars = (max_width / (8 * scale)).max(1) as usize;
    let mut line = String::new();
    let mut line_no = 0;

    for word in text.split_whitespace() {
        if line.len() + word.len() + 1 > max_chars {
            draw_text(fb, x, y + line_no * (10 * scale), scale, color, &line);
            line.clear();
            line_no += 1;
            if line_no >= 3 {
                return;
            }
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }

    if !line.is_empty() && line_no < 3 {
        draw_text(fb, x, y + line_no * (10 * scale), scale, color, &line);
    }
}

fn draw_text(fb: &mut [u32], x: i32, y: i32, scale: i32, color: u32, text: &str) {
    let mut cursor_x = x;
    for ch in text.chars() {
        draw_char(fb, cursor_x, y, scale, color, ch);
        cursor_x += 8 * scale;
    }
}

fn draw_char(fb: &mut [u32], x: i32, y: i32, scale: i32, color: u32, ch: char) {
    let glyph = BASIC_FONTS.get(ch).or_else(|| BASIC_FONTS.get('?'));
    let Some(glyph) = glyph else {
        return;
    };

    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..8 {
            if ((bits >> col) & 1) == 0 {
                continue;
            }
            let px = x + (col * scale as usize) as i32;
            let py = y + (row * scale as usize) as i32;
            draw_rect(fb, px, py, scale, scale, color);
        }
    }
}

fn draw_outline(fb: &mut [u32], x: i32, y: i32, w: i32, h: i32, color: u32, thickness: i32) {
    draw_rect(fb, x, y, w, thickness, color);
    draw_rect(fb, x, y + h - thickness, w, thickness, color);
    draw_rect(fb, x, y, thickness, h, color);
    draw_rect(fb, x + w - thickness, y, thickness, h, color);
}

fn draw_rect(fb: &mut [u32], x: i32, y: i32, w: i32, h: i32, color: u32) {
    if w <= 0 || h <= 0 {
        return;
    }

    let x0 = x.max(0) as usize;
    let y0 = y.max(0) as usize;
    let x1 = (x + w).min(UI_WIDTH as i32).max(0) as usize;
    let y1 = (y + h).min(UI_HEIGHT as i32).max(0) as usize;

    if x0 >= x1 || y0 >= y1 {
        return;
    }

    for py in y0..y1 {
        let row = py * UI_WIDTH;
        for px in x0..x1 {
            fb[row + px] = color;
        }
    }
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    let af = a as f32;
    let bf = b as f32;
    (af + (bf - af) * t).round() as u8
}

fn pack_rgb(r: u8, g: u8, b: u8) -> u32 {
    0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}
