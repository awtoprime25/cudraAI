use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_cudrac")
}

fn write_tmp(name: &str, src: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("cudrac_perf_{}_{}.cdr", std::process::id(), name));
    fs::write(&path, src).unwrap();
    path
}

// Em `cargo test` normal (debug) o `cudrac` não está otimizado e estes
// programas facilmente passam de 2s sem que isso signifique nada; o limite
// só é imposto em release (como corre `cargo test --release`).
fn check_under_2s(name: &str, src: &str) {
    let path = write_tmp(name, src);
    let t0 = Instant::now();
    let out = Command::new(bin())
        .arg("check")
        .arg(&path)
        .output()
        .unwrap();
    let dt = t0.elapsed();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{name}: esperava-se exit=0, veio {:?}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    eprintln!("perf {name}: {dt:?}");
    if !cfg!(debug_assertions) {
        assert!(
            dt < Duration::from_secs(2),
            "{name}: demorou {dt:?}, esperava-se <2s"
        );
    }
    let _ = fs::remove_file(&path);
}

#[test]
fn perf_100k_lines_increment() {
    let mut src = String::from("x = 0\n");
    for _ in 0..100_000 {
        src.push_str("x = x + 1\n");
    }
    src.push_str("print(x)\n");
    check_under_2s("100k_lines_increment", &src);
}

#[test]
fn perf_10k_function_chain() {
    let mut src = String::new();
    for i in 0..10_000 {
        src.push_str(&format!(
            "def f{i}(n: int) -> int:\n    return f{}(n)\n",
            i + 1
        ));
    }
    src.push_str("def f10000(n: int) -> int:\n    return n\n");
    src.push_str("print(f0(1))\n");
    check_under_2s("10k_function_chain", &src);
}

#[test]
fn perf_100k_function_chain() {
    let mut src = String::new();
    for i in 0..100_000 {
        src.push_str(&format!(
            "def f{i}(n: int) -> int:\n    return f{}(n)\n",
            i + 1
        ));
    }
    src.push_str("def f100000(n: int) -> int:\n    return n\n");
    src.push_str("print(f0(1))\n");
    check_under_2s("100k_function_chain", &src);
}

#[test]
fn perf_1000_level_nested_expr() {
    // o parser recusa a partir de 999 níveis de profundidade (proteção
    // contra stack overflow); 998 operadores + 1 folha fica mesmo no limite.
    let mut src = String::from("x = 1\nprint(");
    for _ in 0..998 {
        src.push('-');
    }
    src.push_str("x)\n");
    check_under_2s("1000_level_nested_expr", &src);
}

#[test]
fn perf_5000_vars_5000_ifs() {
    let mut src = String::new();
    for i in 0..5000 {
        src.push_str(&format!("v{i} = {i}\n"));
    }
    for i in 0..5000 {
        src.push_str(&format!("if v{i} > 5:\n    v{i} = 1\n"));
    }
    src.push_str("print(v0)\n");
    check_under_2s("5000_vars_5000_ifs", &src);
}
