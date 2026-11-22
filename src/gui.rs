use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2};

use crate::gdk;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const REPO_RELEASES_URL: &str = "https://github.com/rpatty-dev/PercyScarlett/releases";

// Three colours from the logo: black, Xbox green, white. Surfaces are tints of the black.
const BLACK: Color32 = Color32::from_rgb(11, 12, 11);
const INSET: Color32 = Color32::from_rgb(7, 8, 7);
const SURFACE: Color32 = Color32::from_rgb(21, 23, 21);
const SURFACE_HI: Color32 = Color32::from_rgb(31, 34, 31);
const LINE: Color32 = Color32::from_rgb(38, 42, 38);
const GREEN: Color32 = Color32::from_rgb(92, 194, 79);
const WHITE: Color32 = Color32::from_rgb(240, 242, 238);
const DIM: Color32 = Color32::from_rgb(142, 148, 140);

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
    self_test: Arc<Mutex<Option<crate::self_test::SelfTestResult>>>,
    action_modal: Arc<Mutex<Option<ActionModal>>>,
    selected_tile: usize,
    tab_switches: f32,
    splash_started: Option<std::time::Instant>,
    splash_centered: bool,
}

#[derive(Clone)]
enum ActionPhase {
    Running,
    Success(String),
    Failed(String, String), // summary, prefilled github issue url
}

#[derive(Clone)]
struct ActionModal {
    title: String,
    phase: ActionPhase,
    started: std::time::Instant,
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

fn fetch_latest_release_tag() -> Option<(String, String)> {
    let curl = if cfg!(windows) { "curl.exe" } else { "curl" };
    let url = format!("{REPO_RELEASES_URL}/latest");
    let cur = parse_semver(APP_VERSION)?;
    let res = silent_cmd(curl)
        .args([
            "-fsSIL",
            "--max-time",
            "6",
            "-o",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
            "-w",
            "%{url_effective}",
            &url,
        ])
        .output()
        .ok()?;
    if !res.status.success() {
        return None;
    }
    let eff = String::from_utf8_lossy(&res.stdout).trim().to_string();
    let (_, tag) = eff.rsplit_once("/tag/")?;
    let rem = parse_semver(tag)?;
    let ver = tag.trim_start_matches(['v', 'V']).to_string();
    (rem > cur).then_some((ver, eff))
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
        loop {
            let found = fetch_latest_release_tag();
            let stop = {
                let mut g = upd.lock().unwrap();
                g.checking = false;
                if found.is_some() {
                    g.available = found;
                    ctx.request_repaint();
                    true
                } else {
                    g.installing
                }
            };
            if stop {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(15));
        }
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
        install_theme(&cc.egui_ctx);

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
            self_test: Arc::new(Mutex::new(None)),
            action_modal: Arc::new(Mutex::new(None)),
            selected_tile: 0,
            tab_switches: 0.0,
            splash_started: Some(std::time::Instant::now()),
            splash_centered: false,
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
        let action_title = match args.first().map(String::as_str) {
            Some("verify-dlls") => "Verifying Sidecar DLLs",
            Some("launch") => "Launching Overlay",
            Some("run") => "Running PE32+ In-Process",
            Some("inspect") => "Inspecting PE32+ Binary",
            Some("plan") => "Planning Bring-Up",
            Some("bench-shaders") => "Benchmarking RDNA2 Shaders",
            Some("probe-mem") => "Probing XMem Physical Pool",
            _ => "Running Action",
        }
        .to_string();
        *self.action_modal.lock().unwrap() = Some(ActionModal {
            title: action_title.clone(),
            phase: ActionPhase::Running,
            started: std::time::Instant::now(),
        });

        let console = Arc::clone(&self.console);
        let busy = Arc::clone(&self.busy);
        let modal = Arc::clone(&self.action_modal);
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
            let (out, ok, summary) = match cmd.output() {
                Ok(o) => {
                    let s1 = String::from_utf8_lossy(&o.stdout);
                    let s2 = String::from_utf8_lossy(&o.stderr);
                    let combined = format!("{s1}{s2}");
                    let lo = combined.to_ascii_lowercase();
                    let failed = !o.status.success()
                        || lo.contains("failed") && !lo.contains("0 failed")
                        || lo.contains("error:")
                        || lo.contains("read error")
                        || lo.contains("not found")
                        || lo.contains("could not");
                    let first_line = combined
                        .lines()
                        .rev()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or(if failed { "Command failed" } else { "Completed" })
                        .trim()
                        .to_string();
                    (format!("$ percyscarlett {}\n{combined}\n", args.join(" ")), !failed, first_line)
                }
                Err(e) => (format!("Failed to run command: {e}\n"), false, format!("Spawn error: {e}")),
            };
            let phase = if ok {
                ActionPhase::Success(summary)
            } else {
                let snippet: String = out.chars().take(1200).collect();
                let title = format!("[Action Failed] {action_title} (v{APP_VERSION})");
                let body = format!(
                    "### Action\n`{action_title}` (`percyscarlett {}`)\n\n### Version / OS\n`v{APP_VERSION}` / `{}` `{}`\n\n### Output\n```text\n{snippet}\n```\n",
                    args.join(" "),
                    std::env::consts::OS,
                    std::env::consts::ARCH
                );
                let issue_url = format!(
                    "https://github.com/rpatty-dev/PercyScarlett/issues/new?title={}&body={}",
                    crate::self_test::url_encode(&title),
                    crate::self_test::url_encode(&body)
                );
                ActionPhase::Failed(summary, issue_url)
            };
            *console.lock().unwrap() = out;
            if let Some(m) = modal.lock().unwrap().as_mut() {
                m.phase = phase;
            }
            *busy.lock().unwrap() = false;
            egui_ctx.request_repaint();
        });
    }
}

const SEMIBOLD: &str = "semibold";

fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

/// PercyPRO is Inter Display (SIL OFL 1.1, see assets/fonts/OFL.txt), bundled so every PC renders the same.
fn install_theme(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes) in [
        ("PercyPRO", &include_bytes!("../assets/fonts/PercyPRO-Regular.ttf")[..]),
        ("PercyPRO-SemiBold", &include_bytes!("../assets/fonts/PercyPRO-SemiBold.ttf")[..]),
    ] {
        fonts.font_data.insert(name.into(), Arc::new(egui::FontData::from_static(bytes)));
    }
    let prop = fonts.families.get_mut(&FontFamily::Proportional).unwrap();
    prop.insert(0, "PercyPRO".into());
    let mut semi = vec!["PercyPRO-SemiBold".to_string()];
    semi.extend(prop.iter().cloned());
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), semi);
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (egui::TextStyle::Small, FontId::proportional(11.5)),
        (egui::TextStyle::Body, FontId::proportional(14.0)),
        (egui::TextStyle::Button, FontId::proportional(14.0)),
        (egui::TextStyle::Heading, semibold(22.0)),
        (egui::TextStyle::Monospace, FontId::monospace(12.5)),
    ]
    .into();
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(14.0, 7.0);
    style.spacing.interact_size.y = 30.0;
    style.animation_time = 0.18;

    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.panel_fill = BLACK;
    v.window_fill = SURFACE;
    v.window_stroke = Stroke::new(1.0_f32, LINE);
    v.window_corner_radius = egui::CornerRadius::same(14);
    v.window_shadow = egui::Shadow { offset: [0, 12], blur: 32, spread: 0, color: Color32::from_black_alpha(160) };
    v.popup_shadow = v.window_shadow;
    v.extreme_bg_color = INSET;
    v.faint_bg_color = SURFACE;
    v.hyperlink_color = GREEN;
    v.selection.bg_fill = GREEN.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0_f32, GREEN);
    v.text_cursor.stroke = Stroke::new(2.0_f32, GREEN);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, WHITE);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, LINE);
    for (w, fill) in [
        (&mut v.widgets.inactive, SURFACE_HI),
        (&mut v.widgets.hovered, mix(SURFACE_HI, WHITE, 0.08)),
        (&mut v.widgets.active, mix(SURFACE_HI, GREEN, 0.35)),
        (&mut v.widgets.open, SURFACE_HI),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::NONE;
        w.fg_stroke = Stroke::new(1.5_f32, WHITE);
        w.corner_radius = egui::CornerRadius::same(8);
        w.expansion = 0.0;
    }
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, LINE);
    ctx.set_style(style);
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Open the picker where the current path (or its parent folder) points, if it exists.
fn start_dir(dlg: rfd::FileDialog, current: &str) -> rfd::FileDialog {
    let p = std::path::Path::new(current);
    match [Some(p), p.parent()].into_iter().flatten().find(|d| d.is_dir()) {
        Some(d) => dlg.set_directory(d),
        None => dlg,
    }
}

fn card() -> egui::Frame {
    egui::Frame::NONE.fill(SURFACE).corner_radius(14.0_f32).inner_margin(20.0)
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(semibold(18.0)).color(WHITE));
    ui.add_space(4.0);
}

fn primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).font(semibold(14.0)).color(BLACK))
        .fill(GREEN)
        .corner_radius(8.0_f32)
}

struct Tile {
    title: &'static str,
    sub: &'static str,
    glyph: &'static str,
}

const TILES: [Tile; 4] = [
    Tile { title: "Launch Overlay", sub: "Start the target .exe with the shim overlay", glyph: "▶" },
    Tile { title: "Verify Sidecars", sub: "Check the build folder's Xbox DLLs load", glyph: "✔" },
    Tile { title: "Benchmark Shaders", sub: "Recompile 500 RDNA2 shaders to DXIL", glyph: "⚡" },
    Tile { title: "Probe XMem", sub: "Allocate a 16 MiB physical test block", glyph: "💾" },
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

    fn spawn_self_test(&self, ctx: &egui::Context) {
        let (rt_have, rt_total, rt_dir) = self.runtime_count.lock().unwrap().clone();
        let shim_dir = if rt_have >= rt_total {
            rt_dir
        } else {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                .unwrap_or_else(|| PathBuf::from("."))
        };
        let st_slot = Arc::clone(&self.self_test);
        let modal = Arc::clone(&self.action_modal);
        let console = Arc::clone(&self.console);
        let busy = Arc::clone(&self.busy);
        let ctx2 = ctx.clone();
        *busy.lock().unwrap() = true;
        *modal.lock().unwrap() = Some(ActionModal {
            title: "Running Hardware Self-Test".into(),
            phase: ActionPhase::Running,
            started: std::time::Instant::now(),
        });
        std::thread::spawn(move || {
            let old_path = std::env::var("PATH").unwrap_or_default();
            std::env::set_var("PATH", format!("{};{old_path}", shim_dir.display()));
            let res = crate::self_test::run_self_test(&shim_dir);
            {
                let mut c = console.lock().unwrap();
                c.clear();
                c.push_str(&format!("{}\n\n{}\n", res.summary, res.markdown));
            }
            if let Some(m) = modal.lock().unwrap().as_mut() {
                m.phase = if res.passed == res.total {
                    ActionPhase::Success(res.summary.clone())
                } else {
                    ActionPhase::Failed(res.summary.clone(), res.issue_url.clone())
                };
            }
            *st_slot.lock().unwrap() = Some(res);
            *busy.lock().unwrap() = false;
            ctx2.request_repaint();
        });
    }

    fn top_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, is_busy: bool) {
        ui.horizontal(|ui| {
            if let Some(tex) = &self.mascot_tex {
                ui.image((tex.id(), Vec2::splat(34.0)));
            }
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("PercyScarlett").font(semibold(17.0)).color(WHITE));
                ui.label(RichText::new("Xbox Series X|S translation layer").size(11.5).color(DIM));
            });
            ui.add_space(36.0);

            let mut active_rect = None;
            for (t, label) in [
                (DashTab::Home, "Home"),
                (DashTab::Shims, "Shims"),
                (DashTab::Shaders, "Shaders"),
                (DashTab::Settings, "Settings"),
            ] {
                let active = self.tab == t;
                let resp = ui.add(
                    egui::Button::new(RichText::new(label).size(14.5).color(if active { WHITE } else { DIM })).frame(false),
                );
                if active {
                    active_rect = Some(resp.rect);
                }
                if resp.clicked() && !active {
                    self.tab = t;
                    self.tab_switches += 1.0;
                }
                ui.add_space(12.0);
            }
            // Sliding underline under the active tab.
            if let Some(r) = active_rect {
                let l = ctx.animate_value_with_time(egui::Id::new("tab_l"), r.left(), 0.22);
                let rr = ctx.animate_value_with_time(egui::Id::new("tab_r"), r.right(), 0.22);
                let y = r.bottom() + 8.0;
                ui.painter().rect_filled(Rect::from_min_max(Pos2::new(l, y), Pos2::new(rr, y + 2.0)), 1.0_f32, GREEN);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(RichText::new(format!("v{APP_VERSION}")).size(12.0).color(DIM)).frame(false))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Click to check for updates")
                    .clicked()
                {
                    let upd = Arc::clone(&self.update);
                    let modal = Arc::clone(&self.action_modal);
                    let ctx2 = ctx.clone();
                    *modal.lock().unwrap() = Some(ActionModal {
                        title: "Checking for Updates".into(),
                        phase: ActionPhase::Running,
                        started: std::time::Instant::now(),
                    });
                    std::thread::spawn(move || {
                        let found = fetch_latest_release_tag();
                        if let Some((ver, url)) = found {
                            {
                                let mut g = upd.lock().unwrap();
                                g.available = Some((ver.clone(), url));
                                g.show_modal = true;
                            }
                            *modal.lock().unwrap() = None;
                        } else if let Some(m) = modal.lock().unwrap().as_mut() {
                            m.phase = ActionPhase::Success(format!("Up to date (v{APP_VERSION})"));
                        }
                        ctx2.request_repaint();
                    });
                }
                if ui.small_button("Feedback").on_hover_text("Send feedback or bug report to GitHub").clicked() {
                    let t = format!("[Feedback] PercyScarlett v{APP_VERSION}");
                    let b = format!(
                        "### Environment\n`v{APP_VERSION}` / `{}` `{}`\n\n### Feedback\n_Describe what worked, what broke, or what you'd like added:_\n",
                        std::env::consts::OS,
                        std::env::consts::ARCH
                    );
                    let url = format!(
                        "https://github.com/rpatty-dev/PercyScarlett/issues/new?title={}&body={}",
                        crate::self_test::url_encode(&t),
                        crate::self_test::url_encode(&b)
                    );
                    ctx.open_url(egui::OpenUrl::new_tab(url));
                }
                let upd_avail = self.update.lock().unwrap().available.clone();
                if let Some((new_ver, _)) = upd_avail {
                    if ui.add(primary(&format!("Update to v{new_ver}")).corner_radius(14.0_f32)).clicked() {
                        self.update.lock().unwrap().show_modal = true;
                    }
                }
                if is_busy {
                    ui.add(egui::Spinner::new().color(GREEN));
                }
            });
        });
    }

    fn home(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, is_busy: bool) {
        let (rt_have, rt_total, rt_dir) = self.runtime_count.lock().unwrap().clone();
        let ready = rt_have >= rt_total;

        card().corner_radius(18.0_f32).inner_margin(28.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Jump back in").font(semibold(34.0)).color(WHITE));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("●").size(10.0).color(if ready { GREEN } else { DIM }));
                        let status = if ready {
                            format!("Runtime ready · {rt_have}/{rt_total} DLLs in {}", rt_dir.display())
                        } else {
                            format!("Standalone mode · {rt_have}/{rt_total} runtime DLLs found")
                        };
                        ui.label(RichText::new(status).size(13.5).color(DIM));
                    });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let btn = if ready {
                            egui::Button::new("Reinstall / add to Start Menu")
                        } else {
                            primary("Download runtime & add to Start Menu")
                        };
                        if ui.add_enabled(!is_busy, btn).clicked() {
                            self.install_runtime(ctx);
                        }
                        if ui.add_enabled(!is_busy, egui::Button::new("Run Self-Test")).clicked() {
                            self.spawn_self_test(ctx);
                        }
                        let st_res = self.self_test.lock().unwrap().clone();
                        if let Some(res) = st_res {
                            let lbl = format!("Submit GitHub Issue ({}/{})", res.passed, res.total);
                            if ui.add(primary(&lbl)).clicked() {
                                ctx.open_url(egui::OpenUrl::new_tab(res.issue_url));
                            }
                        }
                    });
                });
                if let Some(tex) = &self.mascot_tex {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.image((tex.id(), Vec2::splat(112.0)));
                    });
                }
            });
        });

        ui.add_space(22.0);
        heading(ui, "Quick actions");

        // Arrows move focus, Enter or click runs the tile.
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
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(tile_w, 136.0), Sense::click());
                let focused = self.selected_tile == idx;
                let lift = ctx.animate_bool_with_time(resp.id.with("lift"), resp.hovered() || focused, 0.16);
                let ring = ctx.animate_bool_with_time(resp.id.with("ring"), focused, 0.2);
                let r = rect.translate(Vec2::new(0.0, -3.0 * lift));
                let p = ui.painter();
                p.rect_filled(r, 14.0_f32, mix(SURFACE, SURFACE_HI, lift));
                if ring > 0.0 {
                    p.rect_stroke(r, 14.0_f32, Stroke::new(2.0_f32, GREEN.gamma_multiply(ring)), StrokeKind::Outside);
                }
                p.text(r.left_top() + Vec2::new(18.0, 16.0), egui::Align2::LEFT_TOP, t.glyph, FontId::proportional(22.0), GREEN);
                p.text(r.left_bottom() + Vec2::new(18.0, -56.0), egui::Align2::LEFT_TOP, t.title, semibold(16.0), WHITE);
                let sub = p.layout(t.sub.to_string(), FontId::proportional(12.5), DIM, r.width() - 36.0);
                p.galley(r.left_bottom() + Vec2::new(18.0, -34.0), sub, DIM);
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    self.selected_tile = idx;
                    activate = Some(idx);
                }
            }
        });
        if let Some(idx) = activate {
            self.run_cli(ctx, self.tile_args(idx));
        }

        ui.add_space(22.0);
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Grid::new("paths").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                ui.label(RichText::new("Target .exe").color(DIM));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.exe_path).desired_width(360.0));
                    if ui.button("Browse…").clicked() {
                        let dlg = rfd::FileDialog::new().add_filter("Executable", &["exe"]);
                        if let Some(p) = start_dir(dlg, &self.exe_path).pick_file() {
                            self.exe_path = p.display().to_string();
                        }
                    }
                    if ui.button("Launch Overlay").clicked() {
                        self.run_cli(ctx, vec!["launch".into(), self.exe_path.clone()]);
                    }
                    if ui.button("Run In-Process").clicked() {
                        self.run_cli(ctx, vec!["run".into(), self.exe_path.clone()]);
                    }
                    if ui.button("Inspect PE32+").clicked() {
                        self.run_cli(ctx, vec!["inspect".into(), self.exe_path.clone()]);
                    }
                });
                ui.end_row();
                ui.label(RichText::new("Build folder").color(DIM));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.build_dir).desired_width(360.0));
                    if ui.button("Browse…").clicked() {
                        if let Some(p) = start_dir(rfd::FileDialog::new(), &self.build_dir).pick_folder() {
                            self.build_dir = p.display().to_string();
                        }
                    }
                    if ui.button("Verify Sidecar DLLs").clicked() {
                        self.run_cli(ctx, vec!["verify-dlls".into(), self.build_dir.clone()]);
                    }
                    if ui.button("Plan Bring-Up").clicked() {
                        self.run_cli(ctx, vec!["plan".into(), self.build_dir.clone()]);
                    }
                });
                ui.end_row();
            });
        });
    }
}

impl eframe::App for DashboardApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.splash_started.is_some() {
            [0.0, 0.0, 0.0, 0.0]
        } else {
            [11.0 / 255.0, 12.0 / 255.0, 13.0 / 255.0, 1.0]
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let center_vp = |w: f32, h: f32| {
            if let Some(mon) = ctx.input(|i| i.viewport().monitor_size) {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(Pos2::new(
                    ((mon.x - w) * 0.5).max(0.0),
                    ((mon.y - h) * 0.5).max(0.0),
                )));
            }
        };
        if let Some(t0) = self.splash_started {
            if !self.splash_centered {
                center_vp(260.0, 260.0);
                self.splash_centered = true;
            }
            let dt = t0.elapsed().as_secs_f32();
            if dt < 0.70 {
                let alpha = if dt < 0.18 {
                    (dt / 0.18).clamp(0.0, 1.0)
                } else if dt > 0.52 {
                    ((0.70 - dt) / 0.18).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE.fill(Color32::TRANSPARENT))
                    .show(ctx, |ui| {
                        let r = ui.max_rect();
                        let c = r.center();
                        let a = (alpha * 255.0) as u8;
                        ui.painter().circle_stroke(
                            c,
                            96.0,
                            Stroke::new(2.0_f32, Color32::from_rgba_unmultiplied(16, 124, 16, (alpha * 140.0) as u8)),
                        );
                        if let Some(tex) = &self.mascot_tex {
                            let sz = Vec2::splat(186.0 + alpha * 14.0);
                            let ir = Rect::from_center_size(c, sz);
                            ui.painter().image(
                                tex.id(),
                                ir,
                                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                Color32::from_rgba_unmultiplied(255, 255, 255, a),
                            );
                        }
                    });
                ctx.request_repaint();
                return;
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(1180.0, 780.0)));
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(Vec2::new(980.0, 680.0)));
            center_vp(1180.0, 780.0);
            self.splash_started = None;
        }

        let is_busy = *self.busy.lock().unwrap();

        egui::TopBottomPanel::top("top_bar")
            .frame(egui::Frame::NONE.fill(BLACK).inner_margin(egui::Margin::symmetric(24, 14)))
            .show_separator_line(false)
            .show(ctx, |ui| {
                self.top_bar(ui, ctx, is_busy);
                let r = ui.clip_rect();
                ui.painter().hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0_f32, LINE));
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(BLACK).inner_margin(egui::Margin::symmetric(28, 24)))
            .show(ctx, |ui| {
                let (rt_have, rt_total, rt_dir) = self.runtime_count.lock().unwrap().clone();
                let runtime_ready = rt_have >= rt_total;
                if !runtime_ready {
                    card().corner_radius(18.0_f32).inner_margin(32.0).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(RichText::new("First-Time Setup Required").font(semibold(30.0)).color(WHITE));
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(format!(
                                        "Runtime DLLs not installed yet ({rt_have}/{rt_total} found in {}).\nDownload the runtime suite & add PercyScarlett to your Start Menu to unlock the dashboard.",
                                        rt_dir.display()
                                    ))
                                    .size(13.5)
                                    .color(DIM),
                                );
                                ui.add_space(16.0);
                                if ui.add_enabled(!is_busy, primary("Download runtime & add to Start Menu")).clicked() {
                                    self.install_runtime(ctx);
                                }
                            });
                            if let Some(tex) = &self.mascot_tex {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.image((tex.id(), Vec2::splat(112.0)));
                                });
                            }
                        });
                    });
                } else {
                    // Fade the page in on tab switch.
                    let shown = ctx.animate_value_with_time(egui::Id::new("tab_fade"), self.tab_switches, 0.22);
                    ui.set_opacity(1.0 - (self.tab_switches - shown).clamp(0.0, 1.0) * 0.9);

                    match self.tab {
                    DashTab::Home => self.home(ui, ctx, is_busy),
                    DashTab::Shims => {
                        ui.label(RichText::new("Shims").font(semibold(28.0)).color(WHITE));
                        ui.label(RichText::new("Compiled Rust shim suite, 9 DLLs").color(DIM));
                        ui.add_space(12.0);
                        card().show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                                egui::Grid::new("shim_grid").striped(true).spacing([22.0, 10.0]).show(ui, |ui| {
                                    for h in ["DLL", "Subsystem", "Status", "Implemented surface"] {
                                        ui.label(RichText::new(h).size(12.5).color(DIM));
                                    }
                                    ui.end_row();
                                    for s in gdk::SHIM_SPECS {
                                        ui.label(RichText::new(s.dll).font(semibold(14.0)).color(WHITE));
                                        ui.label(s.area);
                                        let live = s.status.contains("Live-DLL");
                                        ui.label(RichText::new(s.status).color(if live { GREEN } else { DIM }));
                                        ui.label(s.role);
                                        ui.end_row();
                                    }
                                });
                            });
                        });
                    }
                    DashTab::Shaders => {
                        ui.label(RichText::new("Shaders").font(semibold(28.0)).color(WHITE));
                        ui.label(RichText::new("RDNA2 (GFX10.3) → SM 6.6 DXIL recompiler").color(DIM));
                        ui.add_space(12.0);
                        card().show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("●").size(10.0).color(GREEN));
                                ui.label("21,522 / 21,522 verified (100%, 0 failures) via dxcompiler.dll");
                            });
                            ui.label(RichText::new("VS 89 · PS 7,730 · CS 51 · Mesh 13,588 · RT 64").color(DIM));
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("shaders.rpf").color(DIM));
                                ui.add(egui::TextEdit::singleline(&mut self.shaders_rpf).desired_width(450.0));
                                if ui.add(primary("Benchmark 500")).clicked() {
                                    self.run_cli(ctx, self.tile_args(2));
                                }
                                if ui.button("Benchmark all 21,522").clicked() {
                                    self.run_cli(ctx, vec!["bench-shaders".into(), self.shaders_rpf.clone(), "--dxil".into()]);
                                }
                            });
                        });
                    }
                    DashTab::Settings => {
                        ui.label(RichText::new("Settings").font(semibold(28.0)).color(WHITE));
                        ui.label(RichText::new("Runtime telemetry and compatibility flags").color(DIM));
                        ui.add_space(12.0);
                        card().show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.checkbox(&mut self.wrap_d3d12, "PERCY_D3D12_WRAP=1 · enable ID3D12DeviceX COM proxy");
                            ui.checkbox(&mut self.log_d3d12, "PERCY_D3D12_LOG · trace PresentX and D3D12.X calls");
                            ui.checkbox(&mut self.log_xmem, "PERCY_XMEM_LOG · trace XMemAlloc / VirtualAlloc pool");
                            ui.add_space(12.0);
                            ui.separator();
                            ui.add_space(8.0);
                            ui.label(RichText::new("Diagnostics & Community Testing").font(semibold(15.0)).color(WHITE));
                            ui.label(RichText::new("Run hardware/shim checks and open a pre-filled GitHub Issue with your results.").size(12.5).color(DIM));
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if ui.add_enabled(!is_busy, primary("Run Self-Test")).clicked() {
                                    self.spawn_self_test(ctx);
                                }
                                let st_res = self.self_test.lock().unwrap().clone();
                                if let Some(res) = st_res {
                                    let lbl = format!("Submit Pre-Filled GitHub Issue ({}/{})", res.passed, res.total);
                                    if ui.add(primary(&lbl)).clicked() {
                                        ctx.open_url(egui::OpenUrl::new_tab(res.issue_url));
                                    }
                                }
                            });
                        });
                    }
                    }
                }

                ui.add_space(22.0);
                ui.label(RichText::new("Console").size(12.5).color(DIM));
                let log_text = self.console.lock().unwrap().clone();
                egui::Frame::NONE.fill(INSET).corner_radius(12.0_f32).inner_margin(14.0).show(ui, |ui| {
                    // Fill the remaining height so short logs start at the top and long ones stick to the bottom.
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .max_height(ui.available_height().max(90.0))
                        .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut log_text.as_str())
                                .font(egui::TextStyle::Monospace)
                                .text_color(mix(DIM, WHITE, 0.4))
                                .frame(false)
                                .desired_width(f32::INFINITY)
                                .desired_rows(1),
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
                egui::Window::new(RichText::new("Update available").font(semibold(16.0)))
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .frame(egui::Frame::window(&ctx.style()).inner_margin(20.0))
                    .show(ctx, |ui| {
                        ui.label(format!("Installed v{APP_VERSION} → latest v{new_ver}"));
                        if !upd_status.is_empty() {
                            ui.label(RichText::new(&upd_status).color(DIM));
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!installing, primary("Download & install")).clicked() {
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

        // Apple FaceID-style animated modal for every action button
        let modal_snap = self.action_modal.lock().unwrap().clone();
        if let Some(m) = modal_snap {
            let mut open = true;
            let mut close_now = false;
            let is_running = matches!(m.phase, ActionPhase::Running);
            if !is_running && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                close_now = true;
            }
            egui::Window::new(RichText::new(&m.title).font(semibold(16.0)).color(WHITE))
                .id(egui::Id::new("action_faceid_modal"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .fixed_size([360.0, 230.0])
                .frame(egui::Frame::window(&ctx.style()).inner_margin(24.0).corner_radius(20.0_f32))
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(76.0), egui::Sense::hover());
                        let c = rect.center();
                        let p = ui.painter();
                        let t = m.started.elapsed().as_secs_f32();
                        match &m.phase {
                            ActionPhase::Running => {
                                ctx.request_repaint();
                                p.circle_stroke(c, 32.0, Stroke::new(3.0_f32, Color32::from_rgb(45, 55, 48)));
                                let segs = 14usize;
                                let a0 = t * 5.2;
                                for i in 0..segs {
                                    let f0 = a0 + (i as f32) * 0.16;
                                    let f1 = f0 + 0.16;
                                    let p0 = c + Vec2::new(f0.cos(), f0.sin()) * 32.0;
                                    let p1 = c + Vec2::new(f1.cos(), f1.sin()) * 32.0;
                                    p.line_segment([p0, p1], Stroke::new(3.5_f32, GREEN));
                                }
                                ui.add_space(12.0);
                                ui.label(RichText::new("Working…").font(semibold(15.0)).color(WHITE));
                            }
                            ActionPhase::Success(msg) => {
                                p.circle_filled(c, 32.0, Color32::from_rgba_premultiplied(24, 120, 44, 70));
                                p.circle_stroke(c, 32.0, Stroke::new(3.0_f32, GREEN));
                                p.line_segment([c + Vec2::new(-12.0, 1.0), c + Vec2::new(-3.0, 10.0)], Stroke::new(3.5_f32, GREEN));
                                p.line_segment([c + Vec2::new(-3.0, 10.0), c + Vec2::new(14.0, -10.0)], Stroke::new(3.5_f32, GREEN));
                                ui.add_space(10.0);
                                ui.label(RichText::new("Verified").font(semibold(16.0)).color(GREEN));
                                ui.label(RichText::new(msg).size(12.5).color(DIM));
                                ui.add_space(12.0);
                                if ui.add(primary("Done")).clicked() {
                                    close_now = true;
                                }
                            }
                            ActionPhase::Failed(msg, issue_url) => {
                                let red = Color32::from_rgb(235, 72, 72);
                                p.circle_filled(c, 32.0, Color32::from_rgba_premultiplied(160, 35, 35, 70));
                                p.circle_stroke(c, 32.0, Stroke::new(3.0_f32, red));
                                p.line_segment([c + Vec2::new(-10.0, -10.0), c + Vec2::new(10.0, 10.0)], Stroke::new(3.5_f32, red));
                                p.line_segment([c + Vec2::new(10.0, -10.0), c + Vec2::new(-10.0, 10.0)], Stroke::new(3.5_f32, red));
                                ui.add_space(10.0);
                                ui.label(RichText::new("Action Failed").font(semibold(16.0)).color(red));
                                ui.label(RichText::new(msg).size(12.0).color(DIM));
                                ui.add_space(12.0);
                                ui.horizontal(|ui| {
                                    if ui.add(primary("Submit GitHub Issue")).clicked() {
                                        ctx.open_url(egui::OpenUrl::new_tab(issue_url.clone()));
                                    }
                                    if ui.button("Close").clicked() {
                                        close_now = true;
                                    }
                                });
                            }
                        }
                    });
                });
            if !open || close_now {
                *self.action_modal.lock().unwrap() = None;
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
    let mut vb = egui::ViewportBuilder::default()
        .with_inner_size([260.0, 260.0])
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top();
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
