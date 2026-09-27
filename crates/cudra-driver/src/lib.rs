//! Pipeline do `cudrac` numa biblioteca: lexer -> parser -> sema -> TIR ->
//! codegen -> objeto, mais a ligacao contra a staticlib do runtime. O binario
//! `cudrac` usa isto, e os testes de processo e de truncagem chamam-no
//! diretamente (sem passar por processos).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use cudra_diagnostics::render_all;
use cudra_lexer::lex;
use cudra_parser::parse;
use cudra_span::{Interner, SourceMap};

pub mod codegen {
    pub use cudra_codegen_llvm::{emit_llvm_ir, emit_object, Options};
}

/// Falha do pipeline. `Diagnostics` ja vem renderizado (pronto para o stderr,
/// com newline final); `Internal` e um bug do compilador.
#[derive(Clone, Debug)]
pub enum CompileError {
    Diagnostics(String),
    Internal(String),
}

/// Falha de um `build`/`run` completo.
#[derive(Clone, Debug)]
pub enum BuildFailure {
    /// erro de leitura do ficheiro: mensagem + exit code
    Source(String, u8),
    Compile(CompileError),
    /// erro da fase de ligacao
    Link(String),
}

/// lexer + parser + sema + verificacao da TIR: e o que o `check` faz e o que
/// os truncamentos atacam.
pub fn check_source(name: &str, src: &str, color: bool) -> Result<cudra_tir::Module, CompileError> {
    let mut sources = SourceMap::new();
    let file = sources.add(name.to_string(), src.to_string());
    let (tokens, lex_diags) = lex(file, src);
    let mut interner = Interner::new();
    let (module, parse_diags) = parse(file, src, &tokens, &mut interner);
    let diags = if lex_diags.is_empty() {
        parse_diags
    } else {
        lex_diags
    };
    if !diags.is_empty() {
        return Err(CompileError::Diagnostics(render_all(
            &diags, &sources, color,
        )));
    }
    match cudra_sema::check(&module, &interner) {
        Ok(tir) => {
            if let Err(e) = cudra_tir::verify(&tir) {
                return Err(CompileError::Internal(e));
            }
            Ok(tir)
        }
        Err(sema_diags) => Err(CompileError::Diagnostics(render_all(
            &sema_diags,
            &sources,
            color,
        ))),
    }
}

pub fn read_source(path: &Path) -> Result<String, (String, u8)> {
    let bytes = fs::read(path).map_err(|e| {
        (
            format!("erro: não foi possível ler '{}': {e}", path.display()),
            2,
        )
    })?;
    String::from_utf8(bytes).map_err(|e| {
        (
            format!("erro: '{}' não é UTF-8 válido: {e}", path.display()),
            1,
        )
    })
}

pub fn stderr_color() -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal()
}

/// Caminho da staticlib do runtime: `CUDRA_RUNTIME_LIB`, em tempo de execucao,
/// sobrepoe-se ao valor embutido pelo `build.rs`.
pub fn runtime_lib() -> PathBuf {
    match std::env::var_os("CUDRA_RUNTIME_LIB") {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from(env!("CUDRA_RUNTIME_LIB_DEFAULT")),
    }
}

pub fn native_libs() -> Vec<String> {
    env!("CUDRA_RUNTIME_NATIVE_LIBS_DEFAULT")
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// Liga um objeto contra a staticlib do runtime e escreve o executavel.
pub fn link(object: &[u8], out: &Path) -> Result<(), String> {
    let dir = tempfile::tempdir()
        .map_err(|e| format!("erro: não foi possível criar um diretório temporário: {e}"))?;
    let obj = dir.path().join("prog.o");
    fs::write(&obj, object)
        .map_err(|e| format!("erro: não foi possível escrever o objeto: {e}"))?;

    let lib = runtime_lib();
    if !lib.exists() {
        return Err(format!(
            "erro: a biblioteca do runtime não existe: {}",
            lib.display()
        ));
    }

    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let mut cmd = Command::new(&cc);
    cmd.arg(&obj).arg(&lib).args(native_libs());
    cmd.arg("-o").arg(out);
    let probe = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("erro: não encontrei o compilador C '{cc}' para ligar o executável")
        } else {
            format!("erro: falha ao correr '{cc}': {e}")
        }
    })?;
    if !probe.status.success() {
        return Err(format!(
            "erro: a ligação falhou:\n{}",
            String::from_utf8_lossy(&probe.stderr)
        ));
    }
    Ok(())
}

/// Compila `src` e liga o executavel em `out`.
pub fn build_executable(src: &Path, out: &Path, release: bool) -> Result<(), BuildFailure> {
    let text = read_source(src).map_err(|(m, c)| BuildFailure::Source(m, c))?;
    let tir = check_source(&src.display().to_string(), &text, stderr_color())
        .map_err(BuildFailure::Compile)?;
    let opts = codegen::Options { release };
    let object = codegen::emit_object(&tir, &opts).map_err(|e| {
        BuildFailure::Compile(CompileError::Internal(format!("erro no codegen: {e}")))
    })?;
    link(&object, out).map_err(BuildFailure::Link)
}

/// Nome do executavel por omissao: o nome do ficheiro fonte sem extensao, na
/// diretoria atual.
pub fn default_output(src: &Path) -> PathBuf {
    src.file_stem().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(src.file_name().unwrap_or_else(|| std::ffi::OsStr::new("a")))
    })
}
