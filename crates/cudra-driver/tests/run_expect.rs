//! Divergencias documentadas face ao CPython: o stdout e o exit code esperados
//! vivem em `<nome>.<perfil>.out`, com uma ultima linha `# exit: N`.
//! `BLESS=1` regrava os ficheiros.
//!
//! As divergencias sao, todas, consequencia de os inteiros serem i64 fixos (o
//! Python tem bigints) e de o overflow ser detetado em debug:
//! - `+ - * -x abs(b) <<` com overflow: debug -> `OverflowError` + exit 1,
//!   release -> wrapping silencioso (regra 8);
//! - `x // -1` com `x = i64::MIN`: debug -> overflow, release -> MIN;
//! - `2 ** -1`: expoente negativo -> `ValueError` nos dois modos (regra 7);
//! - `(-8.0) ** 0.5`: o CPython devolve um complexo -> erro nos dois modos;
//! - `10.0 ** 400`: `OverflowError` nos dois modos;
//! - `range` com contagem >= 2^63: contador interno em u64 (sem divergencia de
//!   stdout, serve de registo do caso de fronteira).

mod common;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

fn cudrac() -> &'static str {
    env!("CARGO_BIN_EXE_cudrac")
}

#[test]
fn run_expect_cases_match_their_recorded_output() {
    let cases = common::cases_in("tests/run-expect");
    assert!(
        cases.len() >= 5,
        "esperavam-se pelo menos 5 casos em tests/run-expect, há {}",
        cases.len()
    );
    let bless = std::env::var("BLESS").is_ok();

    let work = tempfile::tempdir().expect("diretorio temporario para os executaveis");
    let failures = common::in_parallel(&cases, |case| {
        let name = case.file_stem().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(case).expect("ler o caso");
        let args = common::args_of(&src);
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
                    "{name}[{prof}]: a compilacao falhou\n{}",
                    String::from_utf8_lossy(&built.stderr)
                ));
                continue;
            }
            let mut run = Command::new(&exe);
            run.args(&args);
            let got = common::run_with_timeout(&mut run, Duration::from_secs(30))
                .expect("executar o programa");
            let actual = render(&got.stdout, got.code);
            let expected_path = case.with_file_name(format!("{name}.{prof}.out"));
            if bless {
                std::fs::write(&expected_path, &actual).expect("gravar o resultado esperado");
                continue;
            }
            let expected = std::fs::read_to_string(&expected_path).unwrap_or_else(|_| {
                panic!(
                    "falta {} (corre `BLESS=1 cargo test -p cudra-driver --test run_expect`)",
                    expected_path.display()
                )
            });
            if expected != actual {
                out.push(format!(
                    "{name}[{prof}]: diferente do registado\n--- esperado ---\n{expected}\n--- obtido ---\n{actual}"
                ));
            }
        }
        out
    });

    assert!(failures.is_empty(), "\n\n{}", failures.join("\n\n"));
}

fn render(stdout: &[u8], code: Option<i32>) -> String {
    let mut s = String::from_utf8_lossy(stdout).to_string();
    if !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
    }
    s.push_str(&format!("# exit: {}\n", code.unwrap_or(-1)));
    s
}

#[test]
fn run_expect_files_have_both_profiles() {
    for case in common::cases_in("tests/run-expect") {
        let name = case.file_stem().unwrap().to_string_lossy().to_string();
        for prof in ["debug", "release"] {
            let p = case.with_file_name(format!("{name}.{prof}.out"));
            assert!(Path::new(&p).exists(), "falta {}", p.display());
        }
    }
}
