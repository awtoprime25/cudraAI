//! Compila a staticlib do runtime (`cudra-runtime`) para dentro do OUT_DIR e
//! passa ao driver o caminho absoluto da `.a` e as bibliotecas nativas de que
//! ela precisa. A `.a` e construida SEMPRE em release: o perfil do `cudrac`
//! nao tem nada a ver com o perfil do programa do utilizador. Nunca se procura
//! uma `.a` em `target/debug` nem em `target/release`.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

const FALLBACK_NATIVE_LIBS: &str = "-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc";

fn main() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = crate_dir
        .parent()
        .and_then(Path::parent)
        .expect("o crate do driver tem de estar em <raiz>/crates/cudra-driver")
        .to_path_buf();
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR definido pelo cargo"));
    let rt_dir = out_dir.join("rt");
    let manifest = root.join("Cargo.toml");

    for p in [
        root.join("Cargo.toml"),
        root.join("Cargo.lock"),
        root.join("crates/cudra-runtime/src"),
        root.join("crates/cudra-runtime/Cargo.toml"),
    ] {
        println!("cargo:rerun-if-changed={}", p.display());
    }
    println!("cargo:rerun-if-env-changed=CUDRA_RUNTIME_LIB");

    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());

    let build = rt_cargo(&cargo, &manifest, &rt_dir, false);
    if !build.status.success() {
        panic!(
            "a construcao da staticlib do runtime falhou:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
    }

    let lib = rt_dir.join("release").join("libcudra_runtime.a");
    assert!(
        lib.exists(),
        "a staticlib do runtime nao foi gerada em {}",
        lib.display()
    );

    let probe = rt_cargo(&cargo, &manifest, &rt_dir, true);
    let native = parse_native_libs(&String::from_utf8_lossy(&probe.stderr))
        .unwrap_or_else(|| FALLBACK_NATIVE_LIBS.to_string());

    println!(
        "cargo:rustc-env=CUDRA_RUNTIME_LIB_DEFAULT={}",
        lib.display()
    );
    println!("cargo:rustc-env=CUDRA_RUNTIME_NATIVE_LIBS_DEFAULT={native}");
}

fn rt_cargo(cargo: &str, manifest: &Path, rt_dir: &Path, probe_libs: bool) -> std::process::Output {
    let mut cmd = Command::new(cargo);
    cmd.arg(if probe_libs { "rustc" } else { "build" })
        .arg("-p")
        .arg("cudra-runtime")
        .arg("--lib")
        .arg("--release")
        .arg("--target-dir")
        .arg(rt_dir)
        .arg("--manifest-path")
        .arg(manifest);
    if probe_libs {
        cmd.arg("--").arg("--print=native-static-libs");
    }
    // Sob `cargo clippy` estes apontam para o clippy-driver; o target-dir e o
    // nosso, e as flags do build exterior nao se aplicam ao runtime.
    for var in [
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTC_WRAPPER",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_TARGET_DIR",
        "CARGO_MAKEFLAGS",
    ] {
        cmd.env_remove(var);
    }
    cmd.output()
        .unwrap_or_else(|e| panic!("nao consegui correr `{cargo}`: {e}"))
}

fn parse_native_libs(stderr: &str) -> Option<String> {
    for line in stderr.lines() {
        if let Some(idx) = line.find("native-static-libs:") {
            let libs = line[idx + "native-static-libs:".len()..].trim();
            if !libs.is_empty() {
                return Some(libs.to_string());
            }
        }
    }
    None
}
