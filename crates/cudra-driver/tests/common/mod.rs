//! Apoio aos testes de execucao: correr processos com limite de tempo e
//! preparar o shim do modulo `cudra` para o `python3`.
#![allow(dead_code)]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub struct Ran {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
}

/// Corre o processo com limite de tempo. Duas threads drenam stdout/stderr,
/// para que um programa que escreve muito nunca bloqueie.
pub fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> std::io::Result<Ran> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut out = child.stdout.take().expect("stdout ligado a um pipe");
    let mut err = child.stderr.take().expect("stderr ligado a um pipe");
    let h_out = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = out.read_to_end(&mut v);
        v
    });
    let h_err = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = err.read_to_end(&mut v);
        v
    });

    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status;
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                status = Some(st);
                break;
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    status = child.wait().ok();
                    timed_out = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => return Err(e),
        }
    }
    let stdout = h_out.join().unwrap_or_default();
    let stderr = h_err.join().unwrap_or_default();
    Ok(Ran {
        code: status.and_then(|s| s.code()),
        stdout,
        stderr,
        timed_out,
    })
}

/// Shim que faz `from cudra import arg_int, arg_float` funcionar no CPython.
pub fn python_shim() -> &'static Path {
    static SHIM: OnceLock<PathBuf> = OnceLock::new();
    SHIM.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("cudra-py-shim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("criar o diretorio do shim do python");
        std::fs::write(
            dir.join("cudra.py"),
            "import sys\n\n\ndef arg_int(i):\n    return int(sys.argv[i])\n\n\ndef arg_float(i):\n    return float(sys.argv[i])\n",
        )
        .expect("escrever o shim do python");
        dir
    })
    .as_path()
}

/// `# args: a b c` — linha opcional, antes de qualquer instrucao.
pub fn args_of(text: &str) -> Vec<String> {
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            if let Some(rest) = t.strip_prefix("# args:") {
                return rest.split_whitespace().map(str::to_string).collect();
            }
            continue;
        }
        break;
    }
    Vec::new()
}

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

pub fn cases_in(dir: &str) -> Vec<PathBuf> {
    let path = workspace_root().join(dir);
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&path)
        .unwrap_or_else(|e| panic!("não consegui ler {}: {e}", path.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "cdr").unwrap_or(false))
        .collect();
    cases.sort();
    cases
}

/// Corre os casos em paralelo, devolvendo os relatorios de falha.
pub fn in_parallel<F>(cases: &[PathBuf], work: F) -> Vec<String>
where
    F: Fn(&PathBuf) -> Vec<String> + Sync,
{
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(cases.len().max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let failures: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let work = &work;
    let next = &next;
    let failures = &failures;
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(move || loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let Some(case) = cases.get(i) else { return };
                let found = work(case);
                if !found.is_empty() {
                    failures
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .extend(found);
                }
            });
        }
    });
    let mut guard = failures.lock().unwrap_or_else(|e| e.into_inner());
    std::mem::take(&mut *guard)
}
