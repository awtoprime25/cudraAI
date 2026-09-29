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

/// Linha de comando como o driver a deve mostrar: só os argumentos com espaços
/// levam aspas.
#[cfg(unix)]
fn linha_de_comando(cc: &str, args: &[String]) -> String {
    let mut linha = cc.to_string();
    for arg in args {
        linha.push(' ');
        if arg.contains(' ') {
            linha.push('"');
            linha.push_str(arg);
            linha.push('"');
        } else {
            linha.push_str(arg);
        }
    }
    linha
}

/// A linha `  objeto: <caminho> (…)` do erro, sem o comentário que a segue: é
/// o objeto que a mensagem diz ter guardado.
fn objeto_guardado(err: &str) -> PathBuf {
    let linha = err
        .lines()
        .find_map(|l| l.strip_prefix("  objeto: "))
        .unwrap_or_else(|| panic!("sem linha `objeto:` em {err}"));
    PathBuf::from(
        linha
            .rsplit_once(" (")
            .map_or(linha, |(caminho, _)| caminho),
    )
}

/// Uma falha de ligação tem de mostrar o comando exacto que o driver pediu ao
/// compilador C: sem isso, descobri-lo obriga a um wrapper à volta do `cc`. O
/// objeto tem de sobreviver à falha, senão o comando impresso aponta para um
/// ficheiro que já não existe e não se pode repetir.
#[cfg(unix)]
#[test]
fn failed_link_reports_the_exact_command() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tmp_exe("link_comando");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let argv1 = dir.join("argv1.txt");
    let argv2 = dir.join("argv2.txt");
    let marcador = dir.join("ja_correu");
    let cc = dir.join("cc_falso.sh");
    // O `cc` falso grava os argumentos que recebeu e falha à primeira, como
    // faria um `ld` que não encontra uma biblioteca; à segunda corre bem, para
    // se poder provar que o comando impresso é repetível.
    fs::write(
        &cc,
        format!(
            "#!/bin/sh\nif [ -f '{marcador}' ]; then\n  printf '%s\\n' \"$@\" > '{argv2}'\n  exit 0\nfi\ntouch '{marcador}'\nprintf '%s\\n' \"$@\" > '{argv1}'\nexit 1\n",
            marcador = marcador.display(),
            argv2 = argv2.display(),
            argv1 = argv1.display(),
        ),
    )
    .unwrap();
    fs::set_permissions(&cc, fs::Permissions::from_mode(0o755)).unwrap();

    let src = write_tmp("link_comando_src", b"print(1)\n");
    // Um espaço no caminho da saída obriga a mostrar as aspas.
    let exe = dir.join("saida com espaço").join("prog");
    // O `TMPDIR` é para o objeto que fica guardado cair dentro do diretório do
    // teste, onde o podemos ir buscar.
    let out = Command::new(bin())
        .args(["build", src.to_str().unwrap(), "-o"])
        .arg(&exe)
        .env("CC", &cc)
        .env("TMPDIR", &dir)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(err.starts_with("erro: a ligação falhou:"), "{err}");
    assert!(err.contains("saiu com o código 1"), "{err}");

    // O comando impresso tem de ser, argumento a argumento, o que o `cc`
    // recebeu — é isso que o torna copiável para a consola.
    let recebidos: Vec<String> = fs::read_to_string(&argv1)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    assert!(recebidos[0].ends_with("prog.o"), "{recebidos:?}");
    assert!(
        recebidos.contains(&cudra_driver::runtime_lib().display().to_string()),
        "{recebidos:?}"
    );
    assert!(
        recebidos.contains(&exe.display().to_string()),
        "{recebidos:?}"
    );
    for lib in cudra_driver::native_libs() {
        assert!(recebidos.contains(&lib), "falta {lib} em {recebidos:?}");
    }
    let esperado = linha_de_comando(cc.to_str().unwrap(), &recebidos);
    assert!(
        err.contains(&format!("  comando: {esperado}\n")),
        "stderr inesperado: {err}"
    );

    // O objeto que o erro aponta é o mesmo que o `cc` recebeu, ficou em disco e
    // vive numa pasta temporária reconhecível.
    let objeto = objeto_guardado(&err);
    assert_eq!(objeto, PathBuf::from(&recebidos[0]), "{err}");
    let conteudo = fs::read(&objeto).unwrap_or_else(|e| panic!("{}: {e}", objeto.display()));
    assert!(!conteudo.is_empty(), "objeto vazio");
    if cfg!(target_os = "linux") {
        assert!(
            conteudo.starts_with(b"\x7fELF"),
            "o objeto guardado não é um objeto ELF"
        );
    }
    let pasta = objeto.parent().unwrap();
    assert!(pasta.starts_with(&dir), "{}", pasta.display());
    assert!(
        pasta
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("cudra-link-"),
        "{}",
        pasta.display()
    );

    // E o comando copiado do erro corre tal e qual: o `cc` falso aceita desta
    // vez, e o pedido que recebe é o mesmo da primeira tentativa.
    let repetido = Command::new("sh")
        .arg("-c")
        .arg(&esperado)
        .output()
        .unwrap();
    assert!(
        repetido.status.success(),
        "o comando impresso não correu: {}",
        String::from_utf8_lossy(&repetido.stderr)
    );
    let segunda: Vec<String> = fs::read_to_string(&argv2)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(segunda, recebidos, "a repetição não foi o mesmo pedido");

    let _ = fs::remove_dir_all(&dir);
}

/// Sem compilador C, o erro também diz qual era o comando, e o objeto fica
/// guardado para o comando poder correr assim que o `CC` existir.
#[test]
fn missing_c_compiler_reports_the_command() {
    let src = write_tmp("sem_cc_src", b"print(1)\n");
    let exe = tmp_exe("sem_cc_exe");
    let cc = tmp_exe("cc_que_nao_existe");
    let out = Command::new(bin())
        .args(["build", src.to_str().unwrap(), "-o"])
        .arg(&exe)
        .env("CC", &cc)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(err.contains("não encontrei o compilador C"), "{err}");
    assert!(
        err.contains(&format!("  comando: {} ", cc.display())),
        "stderr inesperado: {err}"
    );

    let objeto = objeto_guardado(&err);
    assert!(objeto.is_file(), "objeto em falta: {}", objeto.display());
    let pasta = objeto.parent().unwrap();
    assert!(
        pasta
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("cudra-link-"),
        "{}",
        pasta.display()
    );
    let _ = fs::remove_dir_all(pasta);
}

/// Uma ligação que passa não pode deixar nenhuma pasta temporária para trás.
#[cfg(unix)]
#[test]
fn successful_link_leaves_no_temporary_files() {
    let sandbox = tmp_exe("link_limpo");
    let _ = fs::remove_dir_all(&sandbox);
    fs::create_dir_all(&sandbox).unwrap();
    let src = write_tmp("link_limpo_src", b"print(7)\n");
    let exe = tmp_exe("link_limpo_exe");
    let out = Command::new(bin())
        .args(["build", src.to_str().unwrap(), "-o"])
        .arg(&exe)
        .env("TMPDIR", &sandbox)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let corrida = Command::new(&exe).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&corrida.stdout), "7\n");
    let sobras: Vec<String> = fs::read_dir(&sandbox)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert!(sobras.is_empty(), "ficaram sobras: {sobras:?}");

    let _ = fs::remove_dir_all(&sandbox);
    let _ = fs::remove_file(&exe);
}
