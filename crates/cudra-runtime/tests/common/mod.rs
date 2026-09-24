// Suporte partilhado pelos testes de integracao que ligam contra a
// libcudra_runtime.a real (ABI C). `cargo test` so constroi o rlib da crate
// (dependencia normal), nunca a staticlib — por isso o harness compila-a ele
// proprio, a partir do codigo atual, num target-dir dedicado dentro de
// CARGO_TARGET_TMPDIR (isolado do target/ do cargo exterior, para nao
// colidir com o lock dele). Nunca reutiliza uma .a antiga de target/debug ou
// target/release: essa era a fonte do bug — testes a ligar contra codigo
// desatualizado e a passar ou falhar sem relacao com as alteracoes atuais.
use std::env;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

static STATIC_LIB: OnceLock<PathBuf> = OnceLock::new();

/// Caminho da libcudra_runtime.a recem-compilada a partir do codigo-fonte
/// atual. So compila uma vez por processo de teste (OnceLock); chamadas
/// seguintes devolvem o mesmo caminho sem recompilar.
pub fn find_static_lib() -> PathBuf {
    STATIC_LIB.get_or_init(build_static_lib).clone()
}

fn build_static_lib() -> PathBuf {
    let manifest_path = format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR"));
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("rt-staticlib");
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());

    let mut cmd = Command::new(&cargo);
    cmd.arg("build")
        .arg("-p")
        .arg("cudra-runtime")
        .arg("--lib")
        .arg("--manifest-path")
        .arg(&manifest_path)
        .arg("--target-dir")
        .arg(&target_dir);
    if !cfg!(debug_assertions) {
        cmd.arg("--release");
    }

    let output = cmd
        .output()
        .expect("falha ao invocar `cargo build` para compilar a staticlib de teste");
    if !output.status.success() {
        panic!(
            "cargo build da staticlib do cudra-runtime falhou:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let lib_path = target_dir.join(profile).join("libcudra_runtime.a");
    assert!(
        lib_path.exists(),
        "cargo build terminou com sucesso mas {lib_path:?} nao existe"
    );
    lib_path
}
