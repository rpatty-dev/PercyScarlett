use std::path::Path;

use anyhow::{Context, Result};

use crate::{Console, Unimplemented};

#[cfg(windows)]
pub fn run(
    exe: &Path,
    unimplemented: Unimplemented,
    strict: bool,
    execute: bool,
    console: Console,
) -> Result<()> {
    use std::sync::Arc;
    use percyport_hle::{HleResolver, HostPolicy};
    use percyport_loader::{BindReport, UnimplementedPolicy, WithTitleDlls};

    // A title that relaunched itself: the old process must be gone first.
    percyport_hle::modules::lifecycle::wait_for_previous();
    let cfg = percyport_core::Config::load()?;
    if execute && exe.file_name().is_some_and(|n| n.eq_ignore_ascii_case("game_scarlett_master.exe")) {
        // Keep the large inventory and compilation workers out of the title's address space.
        let status = std::process::Command::new(std::env::current_exe()?)
            .arg("prepare-shaders")
            .arg(exe)
            .status()
            .context("Iniciando a preparação dos shaders")?;
        anyhow::ensure!(status.success(), "Preparação de shaders interrompida ({status})");
        println!("Preparação concluída. Iniciando o jogo...");
    }
    let original = std::fs::read(exe).with_context(|| format!("reading {}", exe.display()))?;
    let prepared = percyport_loader::prepare(&original)?;
    let hash = prepared.original_hash.to_hex();
    match percyport_game::identify(&hash) {
        Some(build) => println!("build          {} ({hash})", build.label),
        None => println!("build          unknown ({hash})"),
    }

    let images = cfg.cache_subdir("images")?;
    percyport_hle::modules::d3d12x::use_shader_cache(cfg.cache_subdir("shaders")?);
    let image = percyport_loader::load(&prepared, &images)?;
    percyport_hle::runtime::set_main_image(image.base());
    #[cfg(windows)]
    hold_watchdog(image.base(), image.info().size_of_image as usize);
    percyport_hle::runtime::set_image_path(image.base(), exe);
    println!(
        "mapped at      {:#x} (preferred {:#x})",
        image.base(),
        image.info().image_base
    );
    let resolver = HleResolver::new(if strict {
        HostPolicy::none()
    } else {
        HostPolicy::default()
    });
    let wanted = |dll: &str| title_dll(&resolver.host, dll);
    let title_dir = exe.parent().unwrap_or(Path::new("."));
    // A missing manifest leaves id 0 and no storage quota.
    let manifest =
        std::fs::read_to_string(title_dir.join("MicrosoftGame.config")).unwrap_or_default();
    let title = percyport_hle::runtime::Title::from_manifest(&manifest);
    let storage = cfg.data_subdir(&format!("{:08X}/storage", title.id))?;
    println!(
        "title          {:08X}, storage {} ({} MB)",
        title.id,
        storage.display(),
        title.storage_bytes >> 20
    );
    // The console's fixed drives: the package as `G:`, temporary files `T:`.
    percyport_hle::modules::files::mount_at('G', title_dir);
    let temp = cfg.data_subdir(&format!("{:08X}/temp", title.id))?;
    percyport_hle::modules::files::mount_at('T', &temp);
    percyport_hle::runtime::set_title(percyport_hle::runtime::Title { storage, ..title });
    percyport_hle::console::set(match console {
        Console::SeriesX => percyport_hle::console::Console::SeriesX,
        Console::SeriesS => percyport_hle::console::Console::SeriesS,
    });
    println!("console        {}", percyport_hle::console::current().name());
    let dlls = percyport_loader::load_title_dlls(image.info(), title_dir, &images, &wanted)?;
    for (name, dll) in &dlls {
        println!("title dll      {name} at {:#x}", dll.base());
        percyport_hle::runtime::set_image_path(dll.base(), &title_dir.join(name));
    }

    let policy = match unimplemented {
        Unimplemented::Trap => UnimplementedPolicy::Trap,
        Unimplemented::Zero => UnimplementedPolicy::ReturnZero,
    };
    let with_dlls = WithTitleDlls {
        inner: &resolver,
        dlls: &dlls,
    };
    let show = |name: &str, report: BindReport| {
        println!(
            "bound          {name}: {} hle, {} host, {} title, {} missing",
            report.hle,
            report.host,
            report.title,
            report.missing.len()
        );
        for m in &report.missing {
            println!("  missing      {}!{}", m.dll, m.symbol);
        }
    };
    for (name, dll) in &dlls {
        show(name, dll.bind_imports(&with_dlls, policy)?);
    }
    let exe_name = exe.file_name().unwrap_or_default().to_string_lossy();
    show(&exe_name, image.bind_imports(&with_dlls, policy)?);
    if !execute {
        println!("not executing (pass --execute to start the title)");
        return Ok(());
    }
    percyport_loader::report_crashes();
    percyport_loader::log_crashes_to(temp.with_file_name("crashes.log"));
    percyport_hle::runtime::use_console_timers();
    #[cfg(feature = "diagnostics")]
    diagnostics(&image, &cfg);
    let late = LateDlls {
        host: resolver.host.clone(),
        title_dir: title_dir.to_owned(),
        cache_dir: images,
        policy,
        mapped: std::sync::Mutex::new(dlls.clone()),
        lock: LoaderLock::default(),
    };
    percyport_hle::runtime::set_title_dll_loader(Box::new(move |file| {
        late.load(file, show).map_err(|e| format!("{e:#}"))
    }));
    // SAFETY: imports are bound; from here on we run title code.
    let main = unsafe { Arc::new(image).spawn_main(dlls)? };
    let code = main
        .join()
        .map_err(|_| anyhow::anyhow!("title main thread panicked"))??;
    println!("title exited with {code}");
    Ok(())
}

/// The debugging aids (README): scheduled stack dumps, watched values and
/// memory saved on a crash.
#[cfg(all(windows, feature = "diagnostics"))]
fn diagnostics(image: &percyport_loader::LoadedImage, cfg: &percyport_core::Config) {
    percyport_loader::dump_stacks_on_schedule();
    percyport_hle::modules::audio::capture::start();
    watch(image.base(), image.info().size_of_image as usize);
    // Debugging aid: `PERCYPORT_CRASH_SAVE=<rva>:<len>,...` (hex) writes those
    // title ranges to `crash-<rva>.bin` in the cache dir on a crash.
    let size = image.info().size_of_image as usize;
    let ranges = std::env::var("PERCYPORT_CRASH_SAVE").unwrap_or_default();
    let hex = |s: &str| usize::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok();
    percyport_loader::save_on_crash(
        ranges
            .split(',')
            .filter_map(|r| r.split_once(':'))
            .filter_map(|(rva, len)| Some((hex(rva)?, hex(len)?)))
            .filter(|&(rva, len)| rva.checked_add(len).is_some_and(|end| end <= size))
            .map(|(rva, len)| {
                let file = cfg.cache_dir.join(format!("crash-{rva:x}.bin"));
                (image.base() + rva, len, file)
            })
            .collect(),
    );
}

/// Debugging aid: `PERCYPORT_WATCH=<rva>,...` (hex) logs the u32 at each
/// title+rva whenever it changes, polled every 5 ms (an engine's state, say).
#[cfg(all(windows, feature = "diagnostics"))]
fn watch(base: usize, size: usize) {
    let Ok(list) = std::env::var("PERCYPORT_WATCH") else {
        return;
    };
    let rvas: Vec<usize> = list
        .split(',')
        .filter_map(|r| usize::from_str_radix(r.trim().trim_start_matches("0x"), 16).ok())
        .filter(|&rva| rva + 4 <= size)
        .collect();
    std::thread::spawn(move || {
        let mut seen = vec![None; rvas.len()];
        loop {
            for (&rva, seen) in rvas.iter().zip(&mut seen) {
                // SAFETY: four bytes inside the mapped image, checked above.
                let value = unsafe { ((base + rva) as *const u32).read_volatile() };
                if *seen != Some(value) {
                    eprintln!("PercyPort: title+{rva:#x} = {value:#x}");
                    *seen = Some(value);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    });
}

/// Grand Theft Auto VI 1.0.2.1 ends itself (a write to address 0) when its
/// frame heartbeat stops for ~30 s, which our slow loads do. Its own switch
/// for holding the watchdog (`+0x2214a90`: `mov byte [+0x71b6330], 1`) is
/// set from here, again and again as the title clears it; only when that
/// code is where expected. `PERCYPORT_WATCHDOG` keeps the watchdog on.
#[cfg(windows)]
fn hold_watchdog(base: usize, size: usize) {
    const SETTER: usize = 0x2214a90;
    const FLAG: usize = 0x71b6330;
    const CODE: [u8; 7] = [0xc6, 0x05, 0x99, 0x18, 0xfa, 0x04, 0x01];
    if std::env::var_os("PERCYPORT_WATCHDOG").is_some() || FLAG >= size || SETTER + 7 > size {
        return;
    }
    // SAFETY: seven bytes inside the mapped image (checked above).
    if unsafe { std::slice::from_raw_parts((base + SETTER) as *const u8, 7) } != CODE {
        return;
    }
    println!("title watchdog held");
    std::thread::spawn(move || loop {
        // SAFETY: the title's own flag byte in its data (the setter's target).
        unsafe { ((base + FLAG) as *mut u8).write_volatile(1) };
        std::thread::sleep(std::time::Duration::from_millis(200));
    });
}

/// DLLs the host or our runtime provide are never taken from the title.
#[cfg(windows)]
fn title_dll(host: &percyport_hle::HostPolicy, dll: &str) -> bool {
    !host.allows(dll) && !percyport_hle::registry().iter().any(|m| m.serves(dll))
}

/// Title DLLs the title loads itself (`LoadLibrary`) that nothing imports,
/// e.g. Grand Theft Auto VI's upscaler behind its FidelityFX loader.
#[cfg(windows)]
struct LateDlls {
    host: percyport_hle::HostPolicy,
    title_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
    policy: percyport_loader::UnimplementedPolicy,
    /// Every title DLL mapped so far, to bind new ones against.
    mapped: std::sync::Mutex<Vec<(String, percyport_loader::LoadedImage)>>,
    lock: LoaderLock,
}

#[cfg(windows)]
impl LateDlls {
    /// Every `LoadLibrary` of the title comes here first. Returns a title
    /// image called `file`, mapping it and the title DLLs it needs first,
    /// binding them and running their `DllMain` on the calling thread, as
    /// the Windows loader would; `None` if `file` is no title image.
    fn load(
        &self,
        file: &str,
        show: impl Fn(&str, percyport_loader::BindReport),
    ) -> Result<Option<usize>> {
        use percyport_hle::runtime::{forget_image, image_named, set_image_path};
        // Waits while another thread initialises a DLL; a `DllMain` that
        // loads another DLL re-enters.
        self.lock.hold(|| {
            if let Some(base) = image_named(file) {
                return Ok(Some(base));
            }
            let new = {
                let mut mapped = self.mapped.lock().unwrap_or_else(|e| e.into_inner());
                let wanted = |dll: &str| title_dll(&self.host, dll) && image_named(dll).is_none();
                let new = percyport_loader::load_title_dll(
                    file,
                    &self.title_dir,
                    &self.cache_dir,
                    &wanted,
                )?;
                let hle = percyport_hle::HleResolver::new(self.host.clone());
                let old = percyport_loader::WithTitleDlls {
                    inner: &hle,
                    dlls: &mapped,
                };
                let resolver = percyport_loader::WithTitleDlls {
                    inner: &old,
                    dlls: &new,
                };
                for (name, dll) in &new {
                    println!(
                        "title dll      {name} at {:#x} (loaded by the title)",
                        dll.base()
                    );
                    show(name, dll.bind_imports(&resolver, self.policy)?);
                    set_image_path(dll.base(), &self.title_dir.join(name));
                }
                // Visible before their `DllMain`s run, which may load more.
                mapped.extend(new.iter().cloned());
                new
            };
            let failed = new.iter().enumerate().find_map(|(i, (name, dll))| {
                // SAFETY: bound above, dependencies attached first.
                let attached = unsafe { dll.attach() };
                attached
                    .err()
                    .map(|e| (i, e.context(format!("attaching {name}"))))
            });
            let Some((i, error)) = failed else {
                return Ok(new.last().map(|(_, dll)| dll.base()));
            };
            // As `LoadLibrary` does when a `DllMain` fails: detach what this
            // load attached (the failed one too) and unmap all of it.
            let bases: Vec<_> = new.iter().map(|(_, d)| d.base()).collect();
            self.mapped
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|(_, d)| !bases.contains(&d.base()));
            for (_, done) in new[..=i].iter().rev() {
                // SAFETY: attached (or failing to) on this thread just now.
                unsafe { done.detach() };
            }
            for (_, dll) in new {
                forget_image(dll.base());
                // SAFETY: detached, and no title code holds it yet.
                unsafe { dll.unload() };
            }
            Err(error)
        })
    }
}

/// The Windows loader lock's shape: one thread at a time, re-entered by
/// the thread that holds it.
#[cfg(windows)]
#[derive(Default)]
struct LoaderLock {
    owner: std::sync::Mutex<(Option<std::thread::ThreadId>, usize)>,
    free: std::sync::Condvar,
}

#[cfg(windows)]
impl LoaderLock {
    fn hold<T>(&self, f: impl FnOnce() -> T) -> T {
        let me = std::thread::current().id();
        let lock = || self.owner.lock().unwrap_or_else(|e| e.into_inner());
        {
            let mut owner = lock();
            while owner.0.is_some_and(|o| o != me) {
                owner = self.free.wait(owner).unwrap_or_else(|e| e.into_inner());
            }
            *owner = (Some(me), owner.1 + 1);
        }
        let result = f();
        let mut owner = lock();
        owner.1 -= 1;
        if owner.1 == 0 {
            owner.0 = None;
            self.free.notify_one();
        }
        result
    }
}

#[cfg(all(test, windows))]
mod tests {
    use std::sync::{Arc, Mutex};

    #[test]
    fn loader_lock_is_reentrant_and_exclusive() {
        let lock = Arc::new(super::LoaderLock::default());
        assert_eq!(lock.hold(|| lock.hold(|| 7)), 7);
        let order = Arc::new(Mutex::new(Vec::new()));
        let other = lock.hold(|| {
            let (theirs, their_order) = (lock.clone(), order.clone());
            let other = std::thread::spawn(move || {
                theirs.hold(|| their_order.lock().unwrap().push(2));
            });
            std::thread::sleep(std::time::Duration::from_millis(50));
            order.lock().unwrap().push(1);
            other
        });
        other.join().unwrap();
        assert_eq!(*order.lock().unwrap(), [1, 2]);
    }
}

#[cfg(not(windows))]
pub fn run(_: &Path, _: Unimplemented, _: bool, _: bool, _: Console) -> Result<()> {
    anyhow::bail!("running titles requires Windows")
}
