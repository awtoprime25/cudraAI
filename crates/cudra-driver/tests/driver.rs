use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_cudrac")
}

fn write_tmp(name: &str, contents: &[u8]) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("cudrac_test_{}_{}.cdr", std::process::id(), name));
    fs::write(&path, contents).unwrap();
    path
}

fn run(args: &[&str]) -> Output {
    Command::new(bin()).args(args).output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

#[test]
fn check_ok_exits_0() {
    let path = write_tmp("check_ok", b"x = 1\n");
    let out = run(&["check", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn check_syntax_error_exits_1() {
    let path = write_tmp("check_err", b"x = \n");
    let out = run(&["check", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(!out.stderr.is_empty());
}

#[test]
fn check_missing_file_exits_2() {
    let out = run(&["check", "/caminho/que/nao/existe/xyz.cdr"]);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("erro: não foi possível ler '/caminho/que/nao/existe/xyz.cdr'"),
        "stderr inesperado: {err}"
    );
}

#[test]
fn check_non_utf8_exits_1() {
    let path = write_tmp("check_binary", &[0xFF, 0xFE, 0x00, 0x01]);
    let out = run(&["check", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("não é UTF-8 válido") && err.contains("check_binary"),
        "stderr inesperado: {err}"
    );
}

fn tmp_exe(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("cudrac_test_{}_{}", std::process::id(), name));
    path
}

#[test]
fn build_without_emit_creates_a_running_executable() {
    let path = write_tmp("build_no_emit", b"print(1)\n");
    let exe = tmp_exe("build_no_emit_exe");
    let out = run(&["build", path.to_str().unwrap(), "-o", exe.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let run_out = Command::new(&exe).output().unwrap();
    assert_eq!(run_out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&run_out.stdout), "1\n");
    let _ = fs::remove_file(&exe);
}

#[test]
fn run_prints_stdout_and_returns_exit_code() {
    let path = write_tmp("run_ok", b"print(2 + 3)\n");
    let out = run(&["run", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n");
}

#[test]
fn run_propagates_exit_1_and_partial_stdout() {
    let path = write_tmp("run_div0", b"print(1)\nx = 0\nprint(5 // x)\n");
    let out = run(&["run", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
    assert!(String::from_utf8_lossy(&out.stderr).contains("ZeroDivisionError"));
}

#[test]
fn run_passes_extra_args() {
    let path = write_tmp(
        "run_args",
        b"from cudra import arg_int\nprint(arg_int(1) * 2)\n",
    );
    let out = run(&["run", path.to_str().unwrap(), "--", "21"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn build_emit_llvm_prints_ir() {
    let path = write_tmp("emit_llvm", b"print(1)\n");
    let out = run(&["build", path.to_str().unwrap(), "--emit=llvm"]);
    assert_eq!(out.status.code(), Some(0));
    let ir = stdout(&out);
    assert!(ir.contains("define i32 @main"), "{ir}");
    assert!(ir.contains("@cudra_print_int"), "{ir}");
}

#[test]
fn build_emit_llvm_on_invalid_source_exits_1_with_empty_stdout() {
    let path = write_tmp("emit_llvm_err", b"x = \n");
    let out = run(&["build", path.to_str().unwrap(), "--emit=llvm"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).is_empty());
    assert!(!out.stderr.is_empty());
}

#[test]
fn build_emit_tokens_ok_exits_0() {
    let path = write_tmp("emit_tokens_ok", b"x = 1\n");
    let out = run(&["build", path.to_str().unwrap(), "--emit=tokens"]);
    assert_eq!(out.status.code(), Some(0));
    insta::assert_snapshot!(stdout(&out));
}

#[test]
fn build_emit_ast_ok_exits_0() {
    let path = write_tmp(
        "emit_ast_ok",
        b"def f(a: int, b: float) -> float:\n    return a + b\n",
    );
    let out = run(&["build", path.to_str().unwrap(), "--emit=ast"]);
    assert_eq!(out.status.code(), Some(0));
    insta::assert_snapshot!(stdout(&out));
}

#[test]
fn build_emit_ast_error_exits_1_and_prints_nothing_on_stdout() {
    let path = write_tmp("emit_ast_err", b"x = \n");
    let out = run(&["build", path.to_str().unwrap(), "--emit=ast"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).is_empty());
    assert!(!out.stderr.is_empty());
}
