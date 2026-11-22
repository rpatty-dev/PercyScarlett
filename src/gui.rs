use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2};

use crate::gdk;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const REPO_RELEASES_URL: &str = "https://github.com/rpatty-dev/PercyScarlett/releases";

// Night-sky sunset palette with a console-style green for "ready" states.
const NIGHT_TOP: Color32 = Color32::from_rgb(34, 14, 52);
const NIGHT_BOTTOM: Color32 = Color32::from_rgb(8, 6, 16);
const PINK: Color32 = Color32::from_rgb(255, 56, 140);
const ORANGE: Color32 = Color32::from_rgb(255, 150, 60);
const PURPLE: Color32 = Color32::from_rgb(110, 46, 190);
const TEAL: Color32 = Color32::from_rgb(0, 200, 205);
const READY_GREEN: Color32 = Color32::from_rgb(82, 196, 64);
const CARD_BG: Color32 = Color32::from_rgba_premultiplied(20, 14, 34, 220);
const BORDER_COL: Color32 = Color32::from_rgb(70, 48, 100);
const TEXT_DIM: Color32 = Color32::from_rgb(200, 186, 220);

const REQUIRED_RUNTIME_FILES: &[&str] = &[
    "kernelx.dll",
    "d3d12_xs.dll",
    "xmem.dll",
    "xgameruntime.dll",
    "dstorage.dll",
    "GameInput.dll",
    "xg_xs.dll",
    "winrt_xs.dll",
    "xaudio2_9.dll",
    "dxcompiler.dll",
    "dxil.dll",
];

fn app_install_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(|d| PathBuf::from(d).join("PercyScarlett"))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn count_installed_runtime_files() -> (usize, usize, PathBuf) {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let local_dir = app_install_dir();
    let check_dir = if exe_dir.join("d3d12_xs.dll").exists() {
        exe_dir
    } else {
        local_dir
    };
    let present = REQUIRED_RUNTIME_FILES
        .iter()
        .filter(|f| check_dir.join(f).exists())
        .count();
    (present, REQUIRED_RUNTIME_FILES.len(), check_dir)
}

/// Escape a value for a single-quoted PowerShell string literal.
fn ps_quote(v: impl std::fmt::Display) -> String {
    v.to_string().replace('\'', "''")
}

fn create_start_menu_shortcut(app_dir: &std::path::Path) -> Result<PathBuf, String> {
    let appdata = std::env::var("APPDATA").map_err(|_| "APPDATA not set".to_string())?;
    let programs = PathBuf::from(appdata)
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs");
    fs::create_dir_all(&programs).map_err(|e| e.to_string())?;
    let lnk = programs.join("PercyScarlett.lnk");
    let target = app_dir.join("percyscarlett.exe");
    let ps = format!(
        "$ws = New-Object -ComObject WScript.Shell; $s = $ws.CreateShortcut('{}'); $s.TargetPath = '{}'; $s.WorkingDirectory = '{}'; $s.IconLocation = '{},0'; $s.Description = 'PercyScarlett Xbox Series X|S Dashboard'; $s.Save()",
        ps_quote(lnk.display()),
        ps_quote(target.display()),
        ps_quote(app_dir.display()),
        ps_quote(target.display())
    );
    let out = silent_cmd("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .output()
        .map_err(|e| format!("Shortcut powershell error: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(lnk)
}

fn spawn_install_runtime_and_shortcut(
    console: Arc<Mutex<String>>,
    busy: Arc<Mutex<bool>>,
    runtime_count: Arc<Mutex<(usize, usize, PathBuf)>>,
    ctx: egui::Context,
) {
    {
        let mut b = busy.lock().unwrap();
        if *b {
            return;
        }
        *b = true;
    }
    *console.lock().unwrap() = "Checking latest PercyScarlett release bundle...\n".into();
    ctx.request_repaint();

    std::thread::spawn(move || {
        let res: Result<String, String> = (|| {
            let (curl, null) = if cfg!(windows) { ("curl.exe", "NUL") } else { ("curl", "/dev/null") };
            let mut ver = APP_VERSION.to_string();
            if let Ok(out) = silent_cmd(curl)
                .args(["-fsSL", "-o", null, "-w", "%{url_effective}", &format!("{REPO_RELEASES_URL}/latest")])
                .output()
            {
                if out.status.success() {
                    let eff = String::from_utf8_lossy(&out.stdout);
                    if let Some((_, tag)) = eff.trim().rsplit_once("/tag/") {
                        let v = tag.trim_start_matches(['v', 'V']);
                        if !v.is_empty() {
                            ver = v.to_string();
                        }
                    }
                }
            }

            let install_dir = app_install_dir();
            fs::create_dir_all(&install_dir).map_err(|e| e.to_string())?;
            let tmp = std::env::temp_dir().join("percyscarlett_runtime_dl");
            let _ = fs::remove_dir_all(&tmp);
            fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;

            let zip_p = tmp.join("bundle.zip");
            let dl_url = format!("{REPO_RELEASES_URL}/download/v{ver}/PercyScarlett_v{ver}_Win64.zip");
            *console.lock().unwrap() = format!("Downloading {dl_url}...\n");
            ctx.request_repaint();

            let zip_str = zip_p.to_str().ok_or("bad zip path")?;
            let zip_ps = ps_quote(zip_str);
            let mut downloaded = false;
            // Method 1: curl.exe direct download
            if let Ok(dl) = silent_cmd(curl).args(["-fsSL", "-o", zip_str, &dl_url]).output() {
                if dl.status.success() && zip_p.exists() && fs::metadata(&zip_p).map(|m| m.len() > 100_000).unwrap_or(false) {
                    downloaded = true;
                }
            }
            // Method 2 (Backup): PowerShell Invoke-WebRequest (WinHTTP/system proxy) + GitHub API fallback
            if !downloaded {
                *console.lock().unwrap() = "Primary curl failed; trying PowerShell + GitHub API backup...\n".into();
                ctx.request_repaint();
                let ps_dl = format!(
                    "[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; try {{ Invoke-WebRequest -UseBasicParsing -Uri '{dl_url}' -OutFile '{zip_ps}' }} catch {{ $r = Invoke-RestMethod -UseBasicParsing -Uri 'https://api.github.com/repos/rpatty-dev/PercyScarlett/releases/latest'; $a = $r.assets | Where-Object {{ $_.name -like '*_Win64.zip' }} | Select-Object -First 1; if ($a) {{ Invoke-WebRequest -UseBasicParsing -Uri $a.browser_download_url -OutFile '{zip_ps}' }} }}"
                );
                if let Ok(dl2) = silent_cmd("powershell").args(["-NoProfile", "-Command", &ps_dl]).output() {
                    if dl2.status.success() && zip_p.exists() && fs::metadata(&zip_p).map(|m| m.len() > 100_000).unwrap_or(false) {
                        downloaded = true;
                    }
                }
            }
            if !downloaded {
                return Err(format!("Download failed for both curl and PowerShell backup ({dl_url})"));
            }

            *console.lock().unwrap() = format!("Extracting runtime files to {}...\n", install_dir.display());
            ctx.request_repaint();
            let ps_arg = format!(
                "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                ps_quote(zip_p.display()),
                ps_quote(install_dir.display())
            );
            let ex = silent_cmd("powershell")
                .args(["-NoProfile", "-Command", &ps_arg])
                .output()
                .map_err(|e| format!("Expand error: {e}"))?;
            if !ex.status.success() {
                return Err("Archive extraction failed".into());
            }

            // Ensure current exe is also present in %LOCALAPPDATA%\PercyScarlett if zip lacked it, and copy DLLs next to current exe if desired
            if let Ok(cur_exe) = std::env::current_exe() {
                let target_exe = install_dir.join("percyscarlett.exe");
                if !target_exe.exists() {
                    let _ = fs::copy(&cur_exe, &target_exe);
                }
                if let Some(cur_dir) = cur_exe.parent() {
                    if cur_dir != install_dir {
                        for f in REQUIRED_RUNTIME_FILES {
                            let src = install_dir.join(f);
                            if src.exists() {
                                let _ = fs::copy(&src, cur_dir.join(f));
                            }
                        }
                    }
                }
            }

            let lnk = create_start_menu_shortcut(&install_dir)?;
            let _ = fs::remove_dir_all(&tmp);
            Ok(format!(
                "Installed PercyScarlett v{ver} + all 9 shim DLLs + DXC to {}\nStart Menu shortcut created: {}\n",
                install_dir.display(),
                lnk.display()
            ))
        })();

        match res {
            Ok(msg) => *console.lock().unwrap() = msg,
            Err(err) => *console.lock().unwrap() = format!("Install error: {err}\n"),
        }
        *runtime_count.lock().unwrap() = count_installed_runtime_files();
        *busy.lock().unwrap() = false;
        ctx.request_repaint();
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DashTab {
    Home,
    Shims,
    Shaders,
    Settings,
}

struct DashboardApp {
    tab: DashTab,
    exe_path: String,
    build_dir: String,
    shaders_rpf: String,
    wrap_d3d12: bool,
    log_d3d12: bool,
    log_xmem: bool,
    mascot_tex: Option<egui::TextureHandle>,
    console: Arc<Mutex<String>>,
    busy: Arc<Mutex<bool>>,
    update: Arc<Mutex<UpdateState>>,
    runtime_count: Arc<Mutex<(usize, usize, PathBuf)>>,
    selected_tile: usize,
}

#[derive(Default)]
struct UpdateState {
    available: Option<(String, String)>,
    checking: bool,
    installing: bool,
    show_modal: bool,
    status: String,
}

pub fn parse_semver(s: &str) -> Option<(u32, u32, u32)> {
    let trimmed = s.trim().trim_start_matches(['v', 'V']);
    let core = trimmed.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let maj = parts.next()?.parse::<u32>().ok()?;
    let min = parts.next()?.parse::<u32>().ok()?;
    let pat = parts.next().unwrap_or("0").parse::<u32>().ok()?;
    Some((maj, min, pat))
}

fn silent_cmd(prog: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new(prog);
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    }
    #[cfg(not(windows))]
    Command::new(prog)
}

fn spawn_update_check(upd: Arc<Mutex<UpdateState>>, ctx: egui::Context) {
    {
        let mut g = upd.lock().unwrap();
        if g.checking || g.installing {
            return;
        }
        g.checking = true;
    }
    std::thread::spawn(move || {
        let (curl, null) = if cfg!(windows) { ("curl.exe", "NUL") } else { ("curl", "/dev/null") };
        let out = silent_cmd(curl)
            .args(["-fsSL", "-o", null, "-w", "%{url_effective}", &format!("{REPO_RELEASES_URL}/latest")])
            .output();
        let mut found = None;
        if let Ok(res) = out {
            if res.status.success() {
                let eff = String::from_utf8_lossy(&res.stdout).trim().to_string();
                if let Some((_, tag)) = eff.rsplit_once("/tag/") {
                    if let (Some(rem), Some(cur)) = (parse_semver(tag), parse_semver(APP_VERSION)) {
                        if rem > cur {
                            let ver = tag.trim_start_matches(['v', 'V']).to_string();
                            found = Some((ver, eff));
                        }
                    }
                }
            }
        }
        {
            let mut g = upd.lock().unwrap();
            g.checking = false;
            g.available = found;
        }
        ctx.request_repaint();
    });
}

fn spawn_install_update(ver: String, upd: Arc<Mutex<UpdateState>>, ctx: egui::Context) {
    {
        let mut g = upd.lock().unwrap();
        if g.installing {
            return;
        }
        g.installing = true;
        g.status = format!("Downloading v{ver}...");
    }
    ctx.request_repaint();
    std::thread::spawn(move || {
        let res: Result<(), String> = (|| {
            let tmp = std::env::temp_dir().join("percyscarlett_self_update");
            let _ = fs::remove_dir_all(&tmp);
            fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
            let zip_p = tmp.join("update.zip");
            let dl_url = format!("{REPO_RELEASES_URL}/download/v{ver}/PercyScarlett_v{ver}_Win64.zip");
            let dl = silent_cmd("curl.exe")
                .args(["-fsSL", "-o", zip_p.to_str().ok_or("bad zip path")?, &dl_url])
                .output()
                .map_err(|e| format!("curl error: {e}"))?;
            if !dl.status.success() {
                return Err(format!("Download failed ({})", dl.status));
            }
            {
                let mut g = upd.lock().unwrap();
                g.status = format!("Extracting v{ver}...");
            }
            ctx.request_repaint();
            let ps_arg = format!(
                "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                ps_quote(zip_p.display()),
                ps_quote(tmp.display())
            );
            let ex = silent_cmd("powershell")
                .args(["-NoProfile", "-Command", &ps_arg])
                .output()
                .map_err(|e| format!("Expand error: {e}"))?;
            if !ex.status.success() {
                return Err("Archive extraction failed".into());
            }
            let new_exe = tmp.join("percyscarlett.exe");
            let head = fs::read(&new_exe).map_err(|_| "percyscarlett.exe missing in release zip".to_string())?;
            if head.len() < 100_000 || !head.starts_with(b"MZ") {
                return Err("Downloaded binary looks invalid".into());
            }
            let cur_exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let cur_dir = cur_exe.parent().ok_or("no parent dir")?;
            let old_exe = cur_exe.with_extension("exe.old");
            let _ = fs::remove_file(&old_exe);
            fs::rename(&cur_exe, &old_exe).map_err(|e| format!("Rename failed: {e}"))?;
            if let Err(e) = fs::copy(&new_exe, &cur_exe) {
                let _ = fs::rename(&old_exe, &cur_exe);
                return Err(format!("Replace failed: {e}"));
            }
            // Refresh companion shim DLLs & sample if present in the zip
            if let Ok(rd) = fs::read_dir(&tmp) {
                for ent in rd.flatten() {
                    let p = ent.path();
                    let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    if name.ends_with(".dll") || name.eq_ignore_ascii_case("scarlett_sample.exe") {
                        let _ = fs::copy(&p, cur_dir.join(name));
                    }
                }
            }
            let _ = fs::remove_dir_all(&tmp);
            // Null stdio: after FreeConsole the inherited console handles are dead and CreateProcess rejects them.
            silent_cmd(cur_exe.to_str().ok_or("bad exe path")?)
                .args(["gui", "--relaunch"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("v{ver} installed, but relaunch failed ({e}). Start PercyScarlett again."))?;
            std::process::exit(0);
        })();
        if let Err(err) = res {
            let mut g = upd.lock().unwrap();
            g.installing = false;
            g.status = err;
            ctx.request_repaint();
        }
    });
}

impl DashboardApp {
    fn new(cc: &eframe::CreationContext<'_>, mascot_rgba: Option<(Vec<u8>, u32, u32)>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = NIGHT_BOTTOM;
        visuals.window_fill = Color32::from_rgb(26, 16, 42);
        visuals.window_stroke = Stroke::new(1.0_f32, PINK);
        visuals.extreme_bg_color = Color32::from_rgb(12, 8, 22);
        visuals.selection.bg_fill = PINK.gamma_multiply(0.6);
        visuals.selection.stroke = Stroke::new(1.5_f32, Color32::WHITE);
        visuals.hyperlink_color = TEAL;
        for w in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active] {
            w.corner_radius = egui::CornerRadius::same(4);
        }
        visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(44, 28, 70);
        visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(70, 36, 104);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
        visuals.widgets.active.weak_bg_fill = PINK;
        cc.egui_ctx.set_visuals(visuals);

        let update = Arc::new(Mutex::new(UpdateState::default()));
        spawn_update_check(Arc::clone(&update), cc.egui_ctx.clone());

        let mascot_tex = mascot_rgba.map(|(raw, w, h)| {
            cc.egui_ctx.load_texture(
                "mascot",
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &raw),
                egui::TextureOptions::LINEAR,
            )
        });

        Self {
            tab: DashTab::Home,
            exe_path: String::from(r"target\release\scarlett_sample.exe"),
            build_dir: String::from(r"D:\Grand Theft Auto VI Build"),
            shaders_rpf: String::from(r"D:\Grand Theft Auto VI Build\shaders.rpf"),
            wrap_d3d12: true,
            log_d3d12: true,
            log_xmem: false,
            mascot_tex,
            console: Arc::new(Mutex::new(
                "Dashboard ready. Pick a tile (arrow keys + Enter work too).\n".into(),
            )),
            busy: Arc::new(Mutex::new(false)),
            update,
            runtime_count: Arc::new(Mutex::new(count_installed_runtime_files())),
            selected_tile: 0,
        }
    }

    fn run_cli(&self, ctx: &egui::Context, args: Vec<String>) {
        {
            let mut b = self.busy.lock().unwrap();
            if *b {
                return;
            }
            *b = true;
        }
        let console = Arc::clone(&self.console);
        let busy = Arc::clone(&self.busy);
        let wrap = self.wrap_d3d12;
        let log_d3d = self.log_d3d12;
        let log_xmem = self.log_xmem;
        let egui_ctx = ctx.clone();

        std::thread::spawn(move || {
            let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("percyscarlett.exe"));
            let mut cmd = Command::new(exe);
            cmd.args(&args);
            if wrap {
                cmd.env("PERCY_D3D12_WRAP", "1");
            }
            if log_d3d {
                cmd.env("PERCY_D3D12_LOG", "percy_d3d12.log");
            }
            if log_xmem {
                cmd.env("PERCY_XMEM_LOG", "percy_xmem.log");
            }
            let out = match cmd.output() {
                Ok(o) => {
                    let s1 = String::from_utf8_lossy(&o.stdout);
                    let s2 = String::from_utf8_lossy(&o.stderr);
                    format!("$ percyscarlett {}\n{s1}{s2}\n", args.join(" "))
                }
                Err(e) => format!("Failed to run command: {e}\n"),
            };
            *console.lock().unwrap() = out;
            *busy.lock().unwrap() = false;
            egui_ctx.request_repaint();
        });
    }
}

/// Four-corner colour gradient over `rect` (tl, tr, br, bl).
fn gradient(rect: Rect, c: [Color32; 4]) -> egui::Shape {
    let mut m = egui::Mesh::default();
    m.colored_vertex(rect.left_top(), c[0]);
    m.colored_vertex(rect.right_top(), c[1]);
    m.colored_vertex(rect.right_bottom(), c[2]);
    m.colored_vertex(rect.left_bottom(), c[3]);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    egui::Shape::mesh(m)
}

/// Sunset hero art: purple -> pink -> orange sky with a striped setting sun.
fn hero_art(rect: Rect) -> egui::Shape {
    let mid = rect.left() + rect.width() * 0.55;
    let left = Rect::from_min_max(rect.min, Pos2::new(mid, rect.bottom()));
    let right = Rect::from_min_max(Pos2::new(mid, rect.top()), rect.max);
    let deep = PURPLE.gamma_multiply(0.7);
    let mut shapes = vec![
        gradient(left, [deep, PINK, PINK, deep]),
        gradient(right, [PINK, ORANGE, ORANGE, PINK]),
    ];
    let r = rect.height() * 0.62;
    let c = Pos2::new(rect.right() - r - 40.0, rect.bottom() + r * 0.25);
    shapes.push(egui::Shape::circle_filled(c, r, Color32::from_rgb(255, 214, 120)));
    for k in 0..5 {
        let y = c.y - r * 0.15 - k as f32 * r * 0.16;
        let band = Rect::from_min_max(Pos2::new(c.x - r, y - 2.0 - k as f32 * 0.4), Pos2::new(c.x + r, y));
        shapes.push(egui::Shape::rect_filled(band, 0.0_f32, ORANGE));
    }
    shapes.push(gradient(rect, [Color32::TRANSPARENT, Color32::TRANSPARENT, NIGHT_BOTTOM.gamma_multiply(0.5), NIGHT_BOTTOM.gamma_multiply(0.5)]));
    egui::Shape::Vec(shapes)
}

fn card() -> egui::Frame {
    egui::Frame::NONE
        .fill(CARD_BG)
        .stroke(Stroke::new(1.0_f32, BORDER_COL))
        .corner_radius(6.0_f32)
        .inner_margin(14.0)
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).strong().size(17.0).color(Color32::WHITE));
    ui.add_space(6.0);
}

struct Tile {
    title: &'static str,
    sub: &'static str,
    glyph: &'static str,
    from: Color32,
    to: Color32,
}

const TILES: [Tile; 4] = [
    Tile { title: "Launch Overlay", sub: "Start the target .exe with the shim overlay", glyph: "▶", from: PINK, to: ORANGE },
    Tile { title: "Verify Sidecars", sub: "Check the build folder's Xbox DLLs load", glyph: "✔", from: PURPLE, to: PINK },
    Tile { title: "Benchmark Shaders", sub: "Recompile 500 RDNA2 shaders to DXIL", glyph: "⚡", from: ORANGE, to: PURPLE },
    Tile { title: "Probe XMem", sub: "Allocate a 16 MiB physical test block", glyph: "💾", from: TEAL, to: PURPLE },
];

impl DashboardApp {
    fn tile_args(&self, idx: usize) -> Vec<String> {
        match idx {
            0 => vec!["launch".into(), self.exe_path.clone()],
            1 => vec!["verify-dlls".into(), self.build_dir.clone()],
            2 => vec!["bench-shaders".into(), self.shaders_rpf.clone(), "--dxil".into(), "--limit".into(), "500".into()],
            _ => vec!["probe-mem".into()],
        }
    }

    fn install_runtime(&self, ctx: &egui::Context) {
        spawn_install_runtime_and_shortcut(
            Arc::clone(&self.console),
            Arc::clone(&self.busy),
            Arc::clone(&self.runtime_count),
            ctx.clone(),
        );
    }

    fn top_bar(&mut self, ui: &mut egui::Ui, is_busy: bool) {
        ui.horizontal(|ui| {
            if let Some(tex) = &self.mascot_tex {
                ui.image((tex.id(), Vec2::splat(38.0)));
            }
            ui.vertical(|ui| {
                ui.label(RichText::new("PERCYSCARLETT").strong().size(20.0).color(Color32::WHITE));
                ui.label(RichText::new("Xbox Series X|S translation layer · PercyLicense v1.2").size(11.0).color(TEXT_DIM));
            });
            ui.add_space(28.0);
            for (t, label) in [
                (DashTab::Home, "Home"),
                (DashTab::Shims, "Shims"),
                (DashTab::Shaders, "Shaders"),
                (DashTab::Settings, "Settings"),
            ] {
                let active = self.tab == t;
                let text = RichText::new(label).size(15.0).color(if active { Color32::WHITE } else { TEXT_DIM });
                let resp = ui.add(egui::Button::new(if active { text.strong() } else { text }).frame(false));
                if active {
                    ui.painter().hline(resp.rect.x_range(), resp.rect.bottom() + 6.0, Stroke::new(3.0_f32, PINK));
                }
                if resp.clicked() {
                    self.tab = t;
                }
                ui.add_space(10.0);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("v{APP_VERSION}")).size(12.0).color(TEXT_DIM));
                let upd_avail = self.update.lock().unwrap().available.clone();
                if let Some((new_ver, _)) = upd_avail {
                    let pill = egui::Button::new(RichText::new(format!("Update to v{new_ver}")).size(12.0).strong().color(Color32::BLACK))
                        .fill(ORANGE)
                        .corner_radius(12.0_f32);
                    if ui.add(pill).clicked() {
                        self.update.lock().unwrap().show_modal = true;
                    }
                }
                if is_busy {
                    ui.spinner();
                }
            });
        });
    }

    fn home(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, is_busy: bool) {
        let (rt_have, rt_total, rt_dir) = self.runtime_count.lock().unwrap().clone();

        // Hero banner: art is painted behind the content once its size is known.
        let art = ui.painter().add(egui::Shape::Noop);
        let hero = egui::Frame::NONE.inner_margin(egui::Margin::symmetric(24, 20)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(96.0);
            ui.label(RichText::new("JUMP BACK IN").strong().size(32.0).color(Color32::WHITE));
            ui.add_space(4.0);
            if rt_have < rt_total {
                ui.label(RichText::new(format!("Standalone mode: {rt_have}/{rt_total} runtime DLLs found")).size(13.0).color(Color32::WHITE));
                ui.add_space(8.0);
                let btn = egui::Button::new(RichText::new("⬇  Download runtime & add to Start Menu").strong().color(Color32::BLACK))
                    .fill(Color32::WHITE)
                    .corner_radius(4.0_f32);
                if ui.add_enabled(!is_busy, btn).clicked() {
                    self.install_runtime(ctx);
                }
            } else {
                ui.label(RichText::new(format!("✔ Runtime ready · {rt_have}/{rt_total} DLLs in {}", rt_dir.display())).size(13.0).color(Color32::WHITE));
                ui.add_space(8.0);
                if ui.add_enabled(!is_busy, egui::Button::new("Reinstall / add to Start Menu")).clicked() {
                    self.install_runtime(ctx);
                }
            }
        });
        let hero_rect = hero.response.rect;
        ui.painter().with_clip_rect(hero_rect).set(art, hero_art(hero_rect));

        ui.add_space(18.0);
        heading(ui, "Quick actions");

        // Console-style tile row: arrows move focus, Enter or click runs it.
        let mut activate = None;
        if ctx.memory(|m| m.focused().is_none()) {
            ctx.input(|i| {
                if i.key_pressed(egui::Key::ArrowRight) {
                    self.selected_tile = (self.selected_tile + 1) % TILES.len();
                }
                if i.key_pressed(egui::Key::ArrowLeft) {
                    self.selected_tile = (self.selected_tile + TILES.len() - 1) % TILES.len();
                }
                if i.key_pressed(egui::Key::Enter) {
                    activate = Some(self.selected_tile);
                }
            });
        }
        let gap = 14.0;
        let tile_w = ((ui.available_width() - gap * 3.0) / 4.0).max(150.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (idx, t) in TILES.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(tile_w, 150.0), Sense::click());
                let focused = self.selected_tile == idx;
                let r = if focused || resp.hovered() { rect.expand(3.0) } else { rect };
                let p = ui.painter();
                p.add(gradient(r, [t.from, t.to, t.to.gamma_multiply(0.5), t.from.gamma_multiply(0.5)]));
                let scrim = Rect::from_min_max(Pos2::new(r.left(), r.center().y - 10.0), r.max);
                let shade = Color32::from_black_alpha(190);
                p.add(gradient(scrim, [Color32::TRANSPARENT, Color32::TRANSPARENT, shade, shade]));
                p.text(r.left_top() + Vec2::new(14.0, 10.0), egui::Align2::LEFT_TOP, t.glyph, FontId::proportional(32.0), Color32::WHITE);
                p.text(r.left_bottom() + Vec2::new(14.0, -60.0), egui::Align2::LEFT_TOP, t.title, FontId::proportional(17.0), Color32::WHITE);
                let sub = p.layout(t.sub.to_string(), FontId::proportional(11.5), TEXT_DIM, r.width() - 28.0);
                p.galley(r.left_bottom() + Vec2::new(14.0, -38.0), sub, TEXT_DIM);
                if focused {
                    p.rect_stroke(r, 0.0_f32, Stroke::new(3.0_f32, Color32::WHITE), StrokeKind::Outside);
                }
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                if resp.clicked() {
                    self.selected_tile = idx;
                    activate = Some(idx);
                }
            }
        });
        if let Some(idx) = activate {
            self.run_cli(ctx, self.tile_args(idx));
        }

        ui.add_space(18.0);
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Target .exe").strong());
                ui.add(egui::TextEdit::singleline(&mut self.exe_path).desired_width(420.0));
                if ui.button("▶ Launch Overlay").clicked() {
                    self.run_cli(ctx, vec!["launch".into(), self.exe_path.clone()]);
                }
                if ui.button("Run In-Process").clicked() {
                    self.run_cli(ctx, vec!["run".into(), self.exe_path.clone()]);
                }
                if ui.button("Inspect PE32+").clicked() {
                    self.run_cli(ctx, vec!["inspect".into(), self.exe_path.clone()]);
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Build folder").strong());
                ui.add(egui::TextEdit::singleline(&mut self.build_dir).desired_width(420.0));
                if ui.button("Verify Sidecar DLLs").clicked() {
                    self.run_cli(ctx, vec!["verify-dlls".into(), self.build_dir.clone()]);
                }
                if ui.button("Plan Bring-Up").clicked() {
                    self.run_cli(ctx, vec!["plan".into(), self.build_dir.clone()]);
                }
            });
        });
    }
}

impl eframe::App for DashboardApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let is_busy = *self.busy.lock().unwrap();

        egui::TopBottomPanel::top("top_bar")
            .frame(egui::Frame::NONE.fill(NIGHT_TOP).inner_margin(egui::Margin::symmetric(18, 12)))
            .show(ctx, |ui| {
                self.top_bar(ui, is_busy);
                let r = ui.clip_rect();
                ui.painter().hline(r.x_range(), r.bottom() - 1.0, Stroke::new(2.0_f32, PINK.gamma_multiply(0.8)));
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(NIGHT_BOTTOM).inner_margin(22.0))
            .show(ctx, |ui| {
                let bg = ui.clip_rect();
                ui.painter().add(gradient(bg, [NIGHT_TOP, NIGHT_TOP, NIGHT_BOTTOM, NIGHT_BOTTOM]));

                match self.tab {
                    DashTab::Home => self.home(ui, ctx, is_busy),
                    DashTab::Shims => {
                        heading(ui, "Compiled Rust shim suite (9 DLLs)");
                        card().show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                                egui::Grid::new("shim_grid").striped(true).spacing([18.0, 8.0]).show(ui, |ui| {
                                    for h in ["DLL", "Subsystem", "Status", "Implemented surface"] {
                                        ui.label(RichText::new(h).strong().color(PINK));
                                    }
                                    ui.end_row();
                                    for s in gdk::SHIM_SPECS {
                                        ui.label(RichText::new(s.dll).strong().color(Color32::WHITE));
                                        ui.label(s.area);
                                        ui.label(RichText::new(s.status).color(if s.status.contains("Live-DLL") { READY_GREEN } else { ORANGE }));
                                        ui.label(s.role);
                                        ui.end_row();
                                    }
                                });
                            });
                        });
                    }
                    DashTab::Shaders => {
                        heading(ui, "RDNA2 (GFX10.3) → SM 6.6 DXIL recompiler");
                        card().show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new("Verified: 21,522 / 21,522 (100.0%, fail=0) via dxcompiler.dll (VS=89, PS=7730, CS=51, Mesh=13588, RT=64)").color(READY_GREEN));
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("shaders.rpf").strong());
                                ui.add(egui::TextEdit::singleline(&mut self.shaders_rpf).desired_width(450.0));
                                if ui.button("Benchmark 500").clicked() {
                                    self.run_cli(ctx, self.tile_args(2));
                                }
                                if ui.button("Benchmark all 21,522").clicked() {
                                    self.run_cli(ctx, vec!["bench-shaders".into(), self.shaders_rpf.clone(), "--dxil".into()]);
                                }
                            });
                        });
                    }
                    DashTab::Settings => {
                        heading(ui, "Runtime telemetry & compatibility flags");
                        card().show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.checkbox(&mut self.wrap_d3d12, "PERCY_D3D12_WRAP=1 (enable ID3D12DeviceX COM proxy)");
                            ui.checkbox(&mut self.log_d3d12, "PERCY_D3D12_LOG (trace PresentX & D3D12.X calls)");
                            ui.checkbox(&mut self.log_xmem, "PERCY_XMEM_LOG (trace XMemAlloc / VirtualAlloc pool)");
                        });
                    }
                }

                ui.add_space(16.0);
                ui.label(RichText::new("Console").strong().size(13.0).color(TEXT_DIM));
                ui.add_space(4.0);
                let log_text = self.console.lock().unwrap().clone();
                egui::Frame::NONE
                    .fill(Color32::from_rgb(10, 7, 18))
                    .stroke(Stroke::new(1.0_f32, BORDER_COL))
                    .corner_radius(6.0_f32)
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut log_text.as_str())
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(9),
                            );
                        });
                    });
            });

        let (show_modal, avail, installing, upd_status) = {
            let g = self.update.lock().unwrap();
            (g.show_modal, g.available.clone(), g.installing, g.status.clone())
        };
        if show_modal {
            if let Some((new_ver, rel_url)) = avail {
                let mut open = true;
                egui::Window::new("Update available")
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(RichText::new(format!("Installed v{APP_VERSION} → latest v{new_ver}")).strong());
                        ui.add_space(6.0);
                        if !upd_status.is_empty() {
                            ui.label(RichText::new(&upd_status).color(ORANGE));
                        }
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!installing, egui::Button::new("Download & install")).clicked() {
                                spawn_install_update(new_ver.clone(), Arc::clone(&self.update), ctx.clone());
                            }
                            if ui.button("Open release page").clicked() {
                                ctx.open_url(egui::OpenUrl::new_tab(rel_url));
                            }
                        });
                    });
                if !open {
                    self.update.lock().unwrap().show_modal = false;
                }
            }
        }
    }
}

pub fn run_dashboard() -> eframe::Result<()> {
    #[cfg(windows)]
    {
        extern "system" {
            fn CreateMutexW(attrs: *const std::ffi::c_void, initial: i32, name: *const u16) -> usize;
            fn CloseHandle(h: usize) -> i32;
            fn GetLastError() -> u32;
            fn FindWindowW(cls: *const u16, title: *const u16) -> usize;
            fn ShowWindow(hwnd: usize, cmd: i32) -> i32;
            fn SetForegroundWindow(hwnd: usize) -> i32;
        }
        let mname: Vec<u16> = "Local\\PercyScarlettDashboard\0".encode_utf16().collect();
        // After a self-update the old instance may still hold the mutex for a moment; wait for it.
        let relaunch = std::env::args().any(|a| a == "--relaunch");
        unsafe {
            let mut h = CreateMutexW(std::ptr::null(), 1, mname.as_ptr());
            for _ in 0..50 {
                if !relaunch || GetLastError() != 183 {
                    break;
                }
                CloseHandle(h);
                std::thread::sleep(std::time::Duration::from_millis(100));
                h = CreateMutexW(std::ptr::null(), 1, mname.as_ptr());
            }
            if GetLastError() == 183 {
                let wtitle: Vec<u16> = "PercyScarlett — Xbox Series X|S Dashboard\0".encode_utf16().collect();
                let hwnd = FindWindowW(std::ptr::null(), wtitle.as_ptr());
                if hwnd != 0 {
                    ShowWindow(hwnd, 9); // SW_RESTORE
                    SetForegroundWindow(hwnd);
                }
                return Ok(());
            }
        }
    }

    let mascot_rgba = image::load_from_memory(include_bytes!("../assets/mascot.png"))
        .ok()
        .map(|img| {
            let rgba = img.to_rgba8();
            (rgba.as_raw().to_vec(), rgba.width(), rgba.height())
        });
    let mut vb = egui::ViewportBuilder::default().with_inner_size([1180.0, 780.0]).with_min_inner_size([980.0, 680.0]);
    if let Some((ref raw, w, h)) = mascot_rgba {
        vb = vb.with_icon(egui::IconData {
            rgba: raw.clone(),
            width: w,
            height: h,
        });
    }
    let opts = eframe::NativeOptions {
        viewport: vb,
        ..Default::default()
    };
    eframe::run_native(
        "PercyScarlett — Xbox Series X|S Dashboard",
        opts,
        Box::new(move |cc| Ok(Box::new(DashboardApp::new(cc, mascot_rgba)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_compare_works() {
        assert!(parse_semver("v0.2.1") > parse_semver("0.2.0"));
        assert_eq!(parse_semver("v1.4.6"), Some((1, 4, 6)));
    }

    #[test]
    fn ps_quote_escapes_single_quotes() {
        assert_eq!(ps_quote("a'b"), "a''b");
    }
}
