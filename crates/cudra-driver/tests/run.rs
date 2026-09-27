//! Oraculo de execucao: cada ficheiro de `tests/run` e compilado em debug e em
//! release, executado, e o stdout + exit code comparados com `python3`.
//! Divergencias deliberadas face ao CPython vivem em `tests/run-expect`.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(30);

fn cudrac() -> &'static str {
    env!("CARGO_BIN_EXE_cudrac")
}

#[test]
fn run_cases_match_python_in_debug_and_release() {
    let cases = common::cases_in("tests/run");
    assert!(
        cases.len() >= 40,
        "esperavam-se pelo menos 40 casos em tests/run, há {}",
        cases.len()
    );

    let python_ok = Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(
        python_ok,
        "o `python3` não existe ou não corre: os testes de tests/run comparam com o CPython e não podem passar em silêncio"
    );

    let work = tempfile::tempdir().expect("diretorio temporario para os executaveis");
    let shim = common::python_shim().to_path_buf();

    let failures = common::in_parallel(&cases, |case| {
        let name = case.file_stem().unwrap().to_string_lossy().to_string();
        let text = std::fs::read_to_string(case).expect("ler o caso");
        let args = common::args_of(&text);

        let py = run_python(case, &args, &shim);
        let Some(py) = py else {
            return vec![format!("{name}: o python3 nao devolveu resultado")];
        };
        if py.timed_out {
            return vec![format!("{name}: o python3 excedeu o tempo limite")];
        }
        let Some(py_code) = py.code else {
            return vec![format!("{name}: o python3 morreu por sinal")];
        };

        let mut out = Vec::new();
        for prof in ["debug", "release"] {
            let exe = work.path().join(format!("{name}_{prof}"));
            let mut build = Command::new(cudrac());
            build.arg("build").arg(case).arg("-o").arg(&exe);
            if prof == "release" {
                build.arg("--release");
            }
            let built = common::run_with_timeout(&mut build, Duration::from_secs(120))
                .expect("correr o cudrac");
            if built.code != Some(0) {
                out.push(format!(
                    "{name}[{prof}]: a compilacao falhou (rc={:?})\n{}",
                    built.code,
                    String::from_utf8_lossy(&built.stderr)
                ));
                continue;
            }
            let mut run = Command::new(&exe);
            run.args(&args);
            let got = match common::run_with_timeout(&mut run, TIMEOUT) {
                Ok(r) => r,
                Err(e) => {
                    out.push(format!("{name}[{prof}]: nao consegui executar: {e}"));
                    continue;
                }
            };
            if got.timed_out {
                out.push(format!("{name}[{prof}]: o programa excedeu o tempo limite"));
                continue;
            }
            if got.stdout != py.stdout {
                out.push(format!(
                    "{name}[{prof}]: stdout diferente\n--- python ---\n{}\n--- nosso ---\n{}",
                    String::from_utf8_lossy(&py.stdout),
                    String::from_utf8_lossy(&got.stdout)
                ));
            }
            if got.code != Some(py_code) {
                out.push(format!(
                    "{name}[{prof}]: exit code {:?}, python {py_code}\nstderr: {}",
                    got.code,
                    String::from_utf8_lossy(&got.stderr)
                ));
            }
        }
        out
    });

    assert!(failures.is_empty(), "\n\n{}", failures.join("\n\n"));
}

fn run_python(case: &Path, args: &[String], shim: &Path) -> Option<common::Ran> {
    let mut cmd = Command::new("python3");
    cmd.arg(case).args(args);
    cmd.env("PYTHONPATH", shim);
    cmd.env("PYTHONHASHSEED", "0");
    common::run_with_timeout(&mut cmd, TIMEOUT).ok()
}

#[test]
fn run_case_list_is_sorted_and_unique() {
    let cases: Vec<PathBuf> = common::cases_in("tests/run");
    let names: Vec<String> = cases
        .iter()
        .map(|p| p.file_stem().unwrap().to_string_lossy().to_string())
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        names, sorted,
        "nomes de tests/run repetidos ou fora de ordem"
    );
}
