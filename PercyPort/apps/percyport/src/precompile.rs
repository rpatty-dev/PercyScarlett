//! Prepare the same build- and adapter-specific DXIL cache used at draw time.
use std::collections::HashSet;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, ensure};
use percyport_hle::modules::d3d12x;

use crate::shader::sources;

#[derive(Default)]
struct Inventory {
    seen: HashSet<blake3::Hash>,
    shaders: Vec<Vec<u8>>,
}

impl Inventory {
    fn add_file(&mut self, path: &Path) -> Result<()> {
        let bytes = std::fs::read(path).with_context(|| format!("Lendo {}", path.display()))?;
        for source in sources(&bytes) {
            let container = source.bytes().with_context(|| {
                format!(
                    "Registro de shader danificado em {} {}",
                    path.display(),
                    source.label()
                )
            })?;
            let data = container.as_ref();
            ensure!(
                data.starts_with(b"DXBC") && data.len() >= 32,
                "Contêiner de shader inválido em {} {}",
                path.display(),
                source.label()
            );
            let len = u32::from_le_bytes(data[24..28].try_into()?) as usize;
            ensure!(
                (32..=data.len()).contains(&len),
                "Tamanho de shader inválido em {}",
                path.display()
            );
            let data = &data[..len];
            if self.seen.insert(blake3::hash(data)) {
                self.shaders.push(data.to_vec());
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct Progress {
    ready: usize,
    rejected: usize,
    percent: usize,
}

impl Progress {
    fn complete(&mut self, ready: bool, total: usize) {
        if ready {
            self.ready += 1;
        } else {
            self.rejected += 1;
        }
        let done = self.ready + self.rejected;
        let percent = 100 * done / total;
        if percent == self.percent {
            return;
        }
        self.percent = percent;
        let message = format!(
            "Shaders {percent:3}% ({done}/{total}) | prontos: {} | incompatíveis: {}",
            self.ready, self.rejected
        );
        if io::stdout().is_terminal() && done < total {
            print!("\r{message}");
            let _ = io::stdout().flush();
        } else if io::stdout().is_terminal() {
            println!("\r{message}");
        } else {
            println!("{message}");
        }
    }
}

pub fn run(exe: &Path) -> Result<()> {
    let cfg = percyport_core::Config::load()?;
    d3d12x::use_shader_cache(cfg.cache_subdir("shaders")?);
    if std::env::var_os("PERCYPORT_SKIP_PRECOMPILE").is_some() {
        return Ok(());
    }
    println!("Procurando shaders na instalação local...");
    let dir = exe
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut inventory = Inventory::default();
    inventory.add_file(&dir.join("shaders.rpf"))?;
    ensure!(
        !inventory.shaders.is_empty(),
        "Nenhum shader encontrado em shaders.rpf. Confira a instalação do jogo."
    );
    inventory.add_file(exe)?;
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("dll"))
            && path.is_file()
        {
            inventory.add_file(&path)?;
        }
    }
    let total = inventory.shaders.len();
    println!("Shaders   0% (0/{total})");
    let next = AtomicUsize::new(0);
    let progress = Mutex::new(Progress::default());
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(6)
        .min(total);
    std::thread::scope(|scope| -> Result<()> {
        let mut threads = Vec::new();
        for _ in 0..workers {
            threads.push(scope.spawn(|| -> Result<()> {
                while let Some(data) = inventory.shaders.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let ready = match d3d12x::prepare_shader(data) {
                        Ok(outcome) => outcome,
                        Err(error) => {
                            next.store(total, Ordering::Relaxed);
                            anyhow::bail!("Falha no cache de shaders: {error}");
                        }
                    };
                    progress.lock().unwrap().complete(ready, total);
                }
                Ok(())
            }));
        }
        for worker in threads {
            worker
                .join()
                .map_err(|_| anyhow::anyhow!("Falha no worker de shaders"))??;
        }
        Ok(())
    })?;
    Ok(())
}
