//! Game library: Scarlett package folders the user has added, their status, and per-game
//! launch options (flag files in the identity overlay).
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    /// Title exe (may not exist yet: the folder can be staged before the exe arrives).
    pub exe: PathBuf,
    pub name: String,
    pub version: String,
    pub identity: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExeState {
    Missing,
    /// No MZ header: still encrypted / packaged. Only a decrypted dump of the user's own copy runs.
    NotPe,
    Ready { unresolved: Vec<String> },
}

/// Per-game options, stored as flag files next to the overlay host.
pub const OPTIONS: &[(&str, &str, bool)] = &[
    // (flag file, label, flag present means option OFF)
    ("percy_no_mirror", "VRAM mirror for console buffers", true),
    ("percy_nosound", "Audio output", true),
    ("percy_mock_xsts", "Offline sign-in token", false),
    ("percy_d3d12_debug", "D3D12 debug layer", false),
];

fn attr(xml: &str, tag: &str, name: &str) -> Option<String> {
    let t = xml.find(&format!("<{tag} "))?; // trailing space: `<Executable ` must not match `<ExecutableList>`
    let rest = &xml[t..];
    let end = rest.find('>')?;
    let el = &rest[..end];
    let k = el.find(&format!("{name}=\""))? + name.len() + 2;
    Some(el[k..].split('"').next()?.to_string())
}

fn config_in(dir: &Path) -> Option<String> {
    ["MicrosoftGame.Config", "MicrosoftGame.config"].iter().find_map(|n| std::fs::read_to_string(dir.join(n)).ok())
}

/// From a title exe or its package folder (reads MicrosoftGame.Config for the exe name).
pub fn from_path(p: &Path) -> Option<Game> {
    let (dir, exe) = if p.is_dir() { (p.to_path_buf(), None) } else { (p.parent()?.to_path_buf(), Some(p.to_path_buf())) };
    let cfg = config_in(&dir).unwrap_or_default();
    let exe = exe.or_else(|| attr(&cfg, "Executable", "Name").map(|n| dir.join(n)))?;
    let name = attr(&cfg, "ShellVisuals", "DefaultDisplayName")
        .or_else(|| exe.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_default();
    Some(Game {
        name,
        version: attr(&cfg, "Identity", "Version").unwrap_or_default(),
        identity: attr(&cfg, "Identity", "Name").unwrap_or_default(),
        exe,
    })
}

impl Game {
    pub fn dir(&self) -> PathBuf {
        self.exe.parent().map(Path::to_path_buf).unwrap_or_default()
    }
    pub fn overlay(&self) -> PathBuf {
        self.dir().join(crate::pe_loader::IDENTITY_DIR)
    }
    pub fn log(&self) -> PathBuf {
        self.overlay().join("percy_run.log")
    }
    /// The overlay exists once `register` has run (wdapp registration itself is checked on launch).
    pub fn staged(&self) -> bool {
        self.exe.file_name().map(|n| self.overlay().join(n).exists()).unwrap_or(false)
    }
    pub fn exe_state(&self) -> ExeState {
        let Ok(data) = std::fs::read(&self.exe) else { return ExeState::Missing };
        let Ok(pe) = crate::pe64::parse_pe64(&data) else { return ExeState::NotPe };
        let unresolved = crate::gdk::summarize_bringup(&pe)
            .lines()
            .map(str::trim)
            .filter(|l| l.contains(".dll (") && !l.contains("HOST WIN32") && !l.contains("SHIM"))
            .map(|l| l.split(' ').next().unwrap_or(l).to_string())
            .collect();
        ExeState::Ready { unresolved }
    }
    pub fn option(&self, flag: &str) -> bool {
        let (_, _, inverted) = OPTIONS.iter().find(|o| o.0 == flag).copied().unwrap_or((flag, "", false));
        self.overlay().join(flag).exists() != inverted
    }
    pub fn set_option(&self, flag: &str, on: bool) -> std::io::Result<()> {
        let inverted = OPTIONS.iter().any(|o| o.0 == flag && o.2);
        let p = self.overlay().join(flag);
        if on != inverted { std::fs::write(p, b"") } else { std::fs::remove_file(p).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }) }
    }
    /// Save data root (percy_data_dir.txt), defaulting to `<game>\PercyData`.
    pub fn data_dir(&self) -> String {
        std::fs::read_to_string(self.overlay().join("percy_data_dir.txt"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| self.dir().join("PercyData").display().to_string())
    }
    pub fn set_data_dir(&self, d: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(d)?;
        std::fs::write(self.overlay().join("percy_data_dir.txt"), d)
    }
    /// Last `n` lines of the run log that matter (crashes, exits, presents, runtime notes).
    pub fn log_tail(&self, n: usize) -> Vec<String> {
        let Ok(s) = std::fs::read_to_string(self.log()) else { return vec![] };
        let keep: Vec<&str> = s.lines().filter(|l| ["[crash]", "[exit", "[present]", "[xuser]", "[fmod]", "[mirror]", "MISS"].iter().any(|k| l.contains(k))).collect();
        keep[keep.len().saturating_sub(n)..].iter().map(|l| l.chars().take(200).collect()).collect()
    }
}

fn store() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("PercyScarlett").join("library.txt")
}

pub fn load() -> Vec<Game> {
    std::fs::read_to_string(store())
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| from_path(Path::new(l.trim())))
        .collect()
}

pub fn save(games: &[Game]) -> std::io::Result<()> {
    let p = store();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, games.iter().map(|g| g.exe.display().to_string() + "\n").collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_package_config_without_exe() {
        let d = std::env::temp_dir().join(format!("percy_lib_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("MicrosoftGame.Config"),
            r#"<Game><Identity Name="Studio.Title" Publisher="CN=x" Version="1.2.3.4" /><ShellVisuals DefaultDisplayName="Some Title" /><ExecutableList><Executable Name="title_master.exe" Id="Game" /></ExecutableList></Game>"#,
        )
        .unwrap();
        let g = from_path(&d).unwrap();
        assert_eq!(g.name, "Some Title");
        assert_eq!(g.version, "1.2.3.4");
        assert_eq!(g.identity, "Studio.Title");
        assert_eq!(g.exe, d.join("title_master.exe"));
        assert_eq!(g.exe_state(), ExeState::Missing);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
