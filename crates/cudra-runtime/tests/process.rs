// Testes de comportamento de processo: compila pequenos programas C que
// ligam a libcudra_runtime.a e verifica stdout/stderr/exit code exatos. A
// staticlib e compilada pelo proprio harness (ver tests/common/mod.rs), nao
// reaproveitada de target/debug ou target/release.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

mod common;
use common::find_static_lib;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("relogio antes de 1970")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "cudra_rt_test_{tag}_{}_{nanos}_{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("criar diretorio temporario");
    dir
}

fn compile_c(source: &str, dir: &Path, name: &str, lib_path: &Path) -> PathBuf {
    let c_path = dir.join(format!("{name}.c"));
    std::fs::write(&c_path, source).expect("escrever fonte C");
    let bin_path = dir.join(name);
    let status = Command::new("cc")
        .arg(&c_path)
        .arg(lib_path)
        .arg("-lm")
        .arg("-lpthread")
        .arg("-ldl")
        .arg("-o")
        .arg(&bin_path)
        .status()
        .expect("falha ao invocar `cc` — precisa de build-essential no WSL");
    assert!(status.success(), "cc falhou a compilar {name}.c");
    bin_path
}

const HEADER: &str = r#"
#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

extern void cudra_rt_init(int32_t argc, const char** argv);
extern int32_t cudra_rt_exit(void);
extern void cudra_print_int(int64_t v);
extern void cudra_print_float(double v);
extern void cudra_print_bool(bool v);
extern void cudra_print_none(void);
extern void cudra_print_sep(void);
extern void cudra_print_end(void);
extern int64_t cudra_ipow(int64_t base, int64_t exp, bool checked);
extern int64_t cudra_arg_int(int64_t i);
extern double cudra_arg_float(int64_t i);
extern void cudra_panic(uint32_t code);
extern double cudra_int_truediv(int64_t a, int64_t b);
extern double cudra_float_floordiv(double a, double b);
extern double cudra_float_mod(double a, double b);
extern double cudra_fpow(double x, double y);
extern int32_t cudra_cmp_int_float(int64_t i, double f);
extern int64_t cudra_float_to_int(double f);
"#;

struct RunResult {
    stdout: String,
    stderr: String,
    code: Option<i32>,
}

fn run(bin: &Path, args: &[&str]) -> RunResult {
    let output = Command::new(bin)
        .args(args)
        .output()
        .expect("falha ao correr o binario compilado");
    RunResult {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code(),
    }
}

// a) print de int/float/bool/none com sep/end -> stdout exato, exit 0.
#[test]
fn a_prints_basic_types() {
    let dir = unique_dir("a");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(void) {{
    cudra_rt_init(0, NULL);
    cudra_print_int(42);
    cudra_print_sep();
    cudra_print_float(0.5);
    cudra_print_sep();
    cudra_print_bool(true);
    cudra_print_sep();
    cudra_print_bool(false);
    cudra_print_sep();
    cudra_print_none();
    cudra_print_end();
    return cudra_rt_exit();
}}
"#
    );
    let bin = compile_c(&src, &dir, "a_prints", &lib);
    let r = run(&bin, &[]);
    assert_eq!(r.stdout, "42 0.5 True False None\n");
    assert_eq!(r.stderr, "");
    assert_eq!(r.code, Some(0));
}

// b) prints e depois cudra_panic(0) -> stdout parcial completo (flush antes
// do erro), stderr exato, exit 1.
#[test]
fn b_panic_flushes_partial_stdout() {
    let dir = unique_dir("b");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(void) {{
    cudra_rt_init(0, NULL);
    cudra_print_int(1);
    cudra_print_sep();
    cudra_print_int(2);
    cudra_print_end();
    cudra_panic(0);
    return 0;
}}
"#
    );
    let bin = compile_c(&src, &dir, "b_panic", &lib);
    let r = run(&bin, &[]);
    assert_eq!(r.stdout, "1 2\n");
    assert_eq!(r.stderr, "ZeroDivisionError: division by zero\n");
    assert_eq!(r.code, Some(1));
}

// c) mais de 1 MiB de output (300k prints) -> tamanho e conteudo exatos.
#[test]
fn c_large_output_exact() {
    let dir = unique_dir("c");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(void) {{
    cudra_rt_init(0, NULL);
    for (int64_t i = 0; i < 300000; i++) {{
        cudra_print_int(i);
        cudra_print_end();
    }}
    return cudra_rt_exit();
}}
"#
    );
    let bin = compile_c(&src, &dir, "c_large", &lib);
    let r = run(&bin, &[]);
    let expected: String = (0..300000i64).map(|i| format!("{i}\n")).collect();
    assert_eq!(r.stdout.len(), expected.len());
    assert_eq!(r.stdout, expected);
    assert_eq!(r.code, Some(0));
}

// d) cudra_arg_int com argv real do processo: sucesso, ValueError exato e
// IndexError.
#[test]
fn d_arg_int_success() {
    let dir = unique_dir("d1");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(int argc, char** argv) {{
    cudra_rt_init(argc, (const char**)argv);
    cudra_print_int(cudra_arg_int(1));
    cudra_print_end();
    return cudra_rt_exit();
}}
"#
    );
    let bin = compile_c(&src, &dir, "d_arg_int_ok", &lib);
    let r = run(&bin, &["  -42 "]);
    assert_eq!(r.stdout, "-42\n");
    assert_eq!(r.stderr, "");
    assert_eq!(r.code, Some(0));
}

#[test]
fn d_arg_int_value_error() {
    let dir = unique_dir("d2");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(int argc, char** argv) {{
    cudra_rt_init(argc, (const char**)argv);
    cudra_print_int(cudra_arg_int(1));
    cudra_print_end();
    return cudra_rt_exit();
}}
"#
    );
    let bin = compile_c(&src, &dir, "d_arg_int_bad", &lib);
    let r = run(&bin, &["1__0"]);
    assert_eq!(r.stdout, "");
    assert_eq!(
        r.stderr,
        "ValueError: invalid literal for int() with base 10: '1__0'\n"
    );
    assert_eq!(r.code, Some(1));
}

#[test]
fn d_arg_int_index_error() {
    let dir = unique_dir("d3");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(int argc, char** argv) {{
    cudra_rt_init(argc, (const char**)argv);
    cudra_print_int(cudra_arg_int(5));
    cudra_print_end();
    return cudra_rt_exit();
}}
"#
    );
    let bin = compile_c(&src, &dir, "d_arg_int_idx", &lib);
    let r = run(&bin, &["  -42 "]);
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "IndexError: list index out of range\n");
    assert_eq!(r.code, Some(1));
}

// d) indices negativos em cudra_arg_int seguem sys.argv[i] do Python:
// i < 0 conta a partir do fim (i += argc); -1 e o ultimo argumento, -argc e
// o proprio nome do programa (argv[0]), e -(argc+1) fica fora do intervalo.
#[test]
fn d_arg_int_negative_index_last_arg() {
    let dir = unique_dir("d4");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(int argc, char** argv) {{
    cudra_rt_init(argc, (const char**)argv);
    cudra_print_int(cudra_arg_int(-1));
    cudra_print_end();
    return cudra_rt_exit();
}}
"#
    );
    let bin = compile_c(&src, &dir, "d_arg_int_neg1", &lib);
    let r = run(&bin, &["-1"]);
    assert_eq!(r.stdout, "-1\n");
    assert_eq!(r.stderr, "");
    assert_eq!(r.code, Some(0));
}

#[test]
fn d_arg_int_negative_index_wraps_to_program_name() {
    let dir = unique_dir("d5");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(int argc, char** argv) {{
    cudra_rt_init(argc, (const char**)argv);
    cudra_print_int(cudra_arg_int(-2));
    cudra_print_end();
    return cudra_rt_exit();
}}
"#
    );
    let bin = compile_c(&src, &dir, "d_arg_int_neg_argc", &lib);
    // com argc=2 (programa + "-1"), -2 aponta para argv[0]: o nome do
    // programa, que nao e um inteiro valido -> ValueError.
    let r = run(&bin, &["-1"]);
    assert_eq!(r.stdout, "");
    assert!(
        r.stderr
            .starts_with("ValueError: invalid literal for int() with base 10: "),
        "stderr inesperado: {:?}",
        r.stderr
    );
    assert_eq!(r.code, Some(1));
}

#[test]
fn d_arg_int_negative_index_out_of_range() {
    let dir = unique_dir("d6");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(int argc, char** argv) {{
    cudra_rt_init(argc, (const char**)argv);
    cudra_print_int(cudra_arg_int(-3));
    cudra_print_end();
    return cudra_rt_exit();
}}
"#
    );
    let bin = compile_c(&src, &dir, "d_arg_int_neg_oor", &lib);
    // argc=2 (programa + "-1"): -(argc+1) = -3 fica fora do intervalo.
    let r = run(&bin, &["-1"]);
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "IndexError: list index out of range\n");
    assert_eq!(r.code, Some(1));
}

// e) cudra_ipow: overflow com checked, wrapping sem checked.
#[test]
fn e_ipow_checked_overflow() {
    let dir = unique_dir("e1");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(void) {{
    cudra_rt_init(0, NULL);
    int64_t v = cudra_ipow(2, 63, true);
    cudra_print_int(v);
    cudra_print_end();
    return cudra_rt_exit();
}}
"#
    );
    let bin = compile_c(&src, &dir, "e_ipow_overflow", &lib);
    let r = run(&bin, &[]);
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "OverflowError: integer overflow\n");
    assert_eq!(r.code, Some(1));
}

#[test]
fn e_ipow_unchecked_wraps() {
    let dir = unique_dir("e2");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(void) {{
    cudra_rt_init(0, NULL);
    int64_t v = cudra_ipow(3, 40, false);
    cudra_print_int(v);
    cudra_print_end();
    return cudra_rt_exit();
}}
"#
    );
    let bin = compile_c(&src, &dir, "e_ipow_wrap", &lib);
    let r = run(&bin, &[]);
    assert_eq!(r.stdout, "-6289078614652622815\n");
    assert_eq!(r.code, Some(0));
}

// g) limite de 4300 digitos do cudra_arg_int: acima do limite -> stdout
// vazio, mensagem exata, exit 1. Zeros a esquerda contam para o total.
#[test]
fn g_arg_int_too_many_digits() {
    let dir = unique_dir("g1");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(int argc, char** argv) {{
    cudra_rt_init(argc, (const char**)argv);
    cudra_print_int(cudra_arg_int(1));
    cudra_print_end();
    return cudra_rt_exit();
}}
"#
    );
    let bin = compile_c(&src, &dir, "g_arg_int_limit", &lib);
    let digits = "1".repeat(4301);
    let r = run(&bin, &[&digits]);
    assert_eq!(r.stdout, "");
    assert_eq!(
        r.stderr,
        "ValueError: Exceeds the limit (4300 digits) for integer string conversion: \
         value has 4301 digits; use sys.set_int_max_str_digits() to increase the limit\n"
    );
    assert_eq!(r.code, Some(1));
}

#[test]
fn g_arg_int_too_many_digits_leading_zeros() {
    let dir = unique_dir("g2");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(int argc, char** argv) {{
    cudra_rt_init(argc, (const char**)argv);
    cudra_print_int(cudra_arg_int(1));
    cudra_print_end();
    return cudra_rt_exit();
}}
"#
    );
    let bin = compile_c(&src, &dir, "g_arg_int_limit_zeros", &lib);
    let digits = format!("{}1", "0".repeat(5000));
    let r = run(&bin, &[&digits]);
    assert_eq!(r.stdout, "");
    assert_eq!(
        r.stderr,
        "ValueError: Exceeds the limit (4300 digits) for integer string conversion: \
         value has 5001 digits; use sys.set_int_max_str_digits() to increase the limit\n"
    );
    assert_eq!(r.code, Some(1));
}

// h) surrogateescape: bytes de argv invalidos como UTF-8 -> cada byte
// individualmente invalido vira o codepoint 0xDC00+byte. O proprio programa
// C constroi o seu argv interno com bytes crus (nao passa pelo argv real do
// processo, que nao aceitaria bytes invalidos de forma portavel).
#[test]
fn h_surrogateescape_single_invalid_byte() {
    let dir = unique_dir("h1");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(void) {{
    static unsigned char bad[] = {{ 0xff, 0x00 }};
    const char* argv[] = {{ "prog", (const char*)bad }};
    cudra_rt_init(2, argv);
    cudra_print_int(cudra_arg_int(1));
    cudra_print_end();
    return cudra_rt_exit();
}}
"#
    );
    let bin = compile_c(&src, &dir, "h_surrogateescape_1", &lib);
    let r = run(&bin, &[]);
    assert_eq!(r.stdout, "");
    assert_eq!(
        r.stderr,
        "ValueError: invalid literal for int() with base 10: '\\udcff'\n"
    );
    assert_eq!(r.code, Some(1));
}

#[test]
fn h_surrogateescape_valid_prefix_then_invalid_byte() {
    let dir = unique_dir("h2");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(void) {{
    static unsigned char bad[] = {{ '1', 0xff, 0x00 }};
    const char* argv[] = {{ "prog", (const char*)bad }};
    cudra_rt_init(2, argv);
    cudra_print_int(cudra_arg_int(1));
    cudra_print_end();
    return cudra_rt_exit();
}}
"#
    );
    let bin = compile_c(&src, &dir, "h_surrogateescape_2", &lib);
    let r = run(&bin, &[]);
    assert_eq!(r.stdout, "");
    assert_eq!(
        r.stderr,
        "ValueError: invalid literal for int() with base 10: '1\\udcff'\n"
    );
    assert_eq!(r.code, Some(1));
}

#[test]
fn h_surrogateescape_incomplete_lead_byte() {
    let dir = unique_dir("h3");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(void) {{
    static unsigned char bad[] = {{ 0xc3, 0x00 }};
    const char* argv[] = {{ "prog", (const char*)bad }};
    cudra_rt_init(2, argv);
    cudra_print_int(cudra_arg_int(1));
    cudra_print_end();
    return cudra_rt_exit();
}}
"#
    );
    let bin = compile_c(&src, &dir, "h_surrogateescape_3", &lib);
    let r = run(&bin, &[]);
    assert_eq!(r.stdout, "");
    assert_eq!(
        r.stderr,
        "ValueError: invalid literal for int() with base 10: '\\udcc3'\n"
    );
    assert_eq!(r.code, Some(1));
}

// f) EPIPE: escrever >1 MiB para um pipe cujo leitor fecha logo — sem panic
// nem mensagem de panic do Rust no stderr (o SIGPIPE fica como comportamento
// C por omissao, por isso nao verificamos o exit code).
#[test]
fn f_epipe_no_rust_panic() {
    let dir = unique_dir("f");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(void) {{
    cudra_rt_init(0, NULL);
    for (int64_t i = 0; i < 400000; i++) {{
        cudra_print_int(i);
        cudra_print_end();
    }}
    return cudra_rt_exit();
}}
"#
    );
    let bin = compile_c(&src, &dir, "f_epipe", &lib);

    let mut child = Command::new(&bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("falha ao lancar o processo");

    // Fecha logo o nosso lado de leitura do stdout do filho, simulando um
    // pipe cujo leitor desapareceu.
    drop(child.stdout.take());

    let mut stderr_text = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut stderr_text);
    }
    let _ = child.wait();

    assert!(
        !stderr_text.contains("panicked"),
        "stderr continha uma mensagem de panic do Rust: {stderr_text:?}"
    );
    assert!(
        !stderr_text.contains("RUST_BACKTRACE"),
        "stderr continha uma mensagem de panic do Rust: {stderr_text:?}"
    );
}

// i) mensagens de panico novas dos helpers numericos (codigos 8 a 15):
// stdout parcial e sempre completo (flush antes do erro), stderr exato,
// exit 1. Para os codigos sem wrapper dedicado nesta ronda (division float
// pura, floordiv/mod de inteiros) chama-se cudra_panic diretamente, tal
// como o teste `b` ja fazia para o codigo 0; os restantes disparam o
// panico atraves do proprio wrapper com um input real que o provoca.
#[test]
fn i_float_div_zero() {
    let dir = unique_dir("i1");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(void) {{
    cudra_rt_init(0, NULL);
    cudra_print_int(1);
    cudra_print_end();
    cudra_panic(8);
    return 0;
}}
"#
    );
    let bin = compile_c(&src, &dir, "i_float_div_zero", &lib);
    let r = run(&bin, &[]);
    assert_eq!(r.stdout, "1\n");
    assert_eq!(r.stderr, "ZeroDivisionError: float division by zero\n");
    assert_eq!(r.code, Some(1));
}

#[test]
fn i_int_floordiv_zero() {
    let dir = unique_dir("i2");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(void) {{
    cudra_rt_init(0, NULL);
    cudra_print_int(1);
    cudra_print_end();
    cudra_panic(9);
    return 0;
}}
"#
    );
    let bin = compile_c(&src, &dir, "i_int_floordiv_zero", &lib);
    let r = run(&bin, &[]);
    assert_eq!(r.stdout, "1\n");
    assert_eq!(
        r.stderr,
        "ZeroDivisionError: integer division or modulo by zero\n"
    );
    assert_eq!(r.code, Some(1));
}

#[test]
fn i_int_mod_zero() {
    let dir = unique_dir("i3");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(void) {{
    cudra_rt_init(0, NULL);
    cudra_print_int(1);
    cudra_print_end();
    cudra_panic(10);
    return 0;
}}
"#
    );
    let bin = compile_c(&src, &dir, "i_int_mod_zero", &lib);
    let r = run(&bin, &[]);
    assert_eq!(r.stdout, "1\n");
    assert_eq!(r.stderr, "ZeroDivisionError: integer modulo by zero\n");
    assert_eq!(r.code, Some(1));
}

#[test]
fn i_float_floordiv_zero() {
    let dir = unique_dir("i4");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(void) {{
    cudra_rt_init(0, NULL);
    cudra_print_int(1);
    cudra_print_end();
    cudra_float_floordiv(1.0, 0.0);
    return 0;
}}
"#
    );
    let bin = compile_c(&src, &dir, "i_float_floordiv_zero", &lib);
    let r = run(&bin, &[]);
    assert_eq!(r.stdout, "1\n");
    assert_eq!(
        r.stderr,
        "ZeroDivisionError: float floor division by zero\n"
    );
    assert_eq!(r.code, Some(1));
}

#[test]
fn i_float_mod_zero() {
    let dir = unique_dir("i5");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(void) {{
    cudra_rt_init(0, NULL);
    cudra_print_int(1);
    cudra_print_end();
    cudra_float_mod(1.0, 0.0);
    return 0;
}}
"#
    );
    let bin = compile_c(&src, &dir, "i_float_mod_zero", &lib);
    let r = run(&bin, &[]);
    assert_eq!(r.stdout, "1\n");
    assert_eq!(r.stderr, "ZeroDivisionError: float modulo\n");
    assert_eq!(r.code, Some(1));
}

#[test]
fn i_zero_neg_power() {
    let dir = unique_dir("i6");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(void) {{
    cudra_rt_init(0, NULL);
    cudra_print_int(1);
    cudra_print_end();
    cudra_fpow(0.0, -1.0);
    return 0;
}}
"#
    );
    let bin = compile_c(&src, &dir, "i_zero_neg_power", &lib);
    let r = run(&bin, &[]);
    assert_eq!(r.stdout, "1\n");
    assert_eq!(
        r.stderr,
        "ZeroDivisionError: 0.0 cannot be raised to a negative power\n"
    );
    assert_eq!(r.code, Some(1));
}

#[test]
fn i_pow_overflow() {
    let dir = unique_dir("i7");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(void) {{
    cudra_rt_init(0, NULL);
    cudra_print_int(1);
    cudra_print_end();
    cudra_fpow(10.0, 400.0);
    return 0;
}}
"#
    );
    let bin = compile_c(&src, &dir, "i_pow_overflow", &lib);
    let r = run(&bin, &[]);
    assert_eq!(r.stdout, "1\n");
    assert_eq!(
        r.stderr,
        "OverflowError: (34, 'Numerical result out of range')\n"
    );
    assert_eq!(r.code, Some(1));
}

#[test]
fn i_neg_base_fractional_pow() {
    let dir = unique_dir("i8");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(void) {{
    cudra_rt_init(0, NULL);
    cudra_print_int(1);
    cudra_print_end();
    cudra_fpow(-2.0, 0.5);
    return 0;
}}
"#
    );
    let bin = compile_c(&src, &dir, "i_neg_base_fractional_pow", &lib);
    let r = run(&bin, &[]);
    assert_eq!(r.stdout, "1\n");
    assert_eq!(
        r.stderr,
        "ValueError: potência de base negativa com expoente fracionário ainda não é suportada\n"
    );
    assert_eq!(r.code, Some(1));
}

// ronda 3b: base negativa finita, expoente fracionario, mas o modulo
// pow(|x|, y) estoura para infinito -> o CPython delega em complex_pow e
// levanta OverflowError (nao a nossa ValueError generica).
#[test]
fn i_complex_pow_overflow() {
    let dir = unique_dir("i9");
    let lib = find_static_lib();
    let src = format!(
        r#"{HEADER}
int main(void) {{
    cudra_rt_init(0, NULL);
    cudra_print_int(1);
    cudra_print_end();
    cudra_fpow(-3.0, 946683.3866556706);
    return 0;
}}
"#
    );
    let bin = compile_c(&src, &dir, "i_complex_pow_overflow", &lib);
    let r = run(&bin, &[]);
    assert_eq!(r.stdout, "1\n");
    assert_eq!(r.stderr, "OverflowError: complex exponentiation\n");
    assert_eq!(r.code, Some(1));
}
