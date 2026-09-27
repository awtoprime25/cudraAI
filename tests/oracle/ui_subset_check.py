"""Cross-checa os fixtures de tests/ui e tests/check-ok contra o CPython.

  tests/check-ok/*.cdr: têm de correr sem erro no Python (são um
    subconjunto válido de Python) e o `cudrac check` tem de aceitar.
  tests/ui/*.cdr: o `cudrac check` tem de rejeitar (o harness em Rust
    já garante isto via os `.stderr` gravados). Além disso, para TODOS os
    ficheiros (independentemente do nome) que o CPython aceita, o `.stderr`
    gravado tem de justificar a rejeição: ou com a frase
    "ainda não é/são suportad" (contrato UNSUP), ou com uma das exceções
    específicas abaixo (restrições da Fase 1 que o CPython não impõe).
    Se o próprio CPython já falhar para esse caso, não há contrato a
    verificar — exceto a frase "usada antes de ser definida", que só é
    permitida quando o próprio CPython falha com NameError (ver ronda 2
    da parte D, ponto 1: essa frase terse implica que o CPython lançaria
    NameError; se não lançar, tem de vir embrulhada em "ainda não é
    suportado" ou nem sequer ser rejeitada).

uso: python3 ui_subset_check.py <cudrac>
"""
import os
import subprocess
import sys
import tempfile

SHIM = "import sys\ndef arg_int(i):\n    return int(sys.argv[i])\ndef arg_float(i):\n    return float(sys.argv[i])\n"
ARGS = ["5", "2.5"]
UNSUP = ("ainda não é suportad", "ainda não são suportad")
ALLOWED_EXCEPTIONS = (
    "tipo incompatível",
    "tipo de retorno incompatível",
    "pode não estar definida",
    "return inconsistente",
    "pode terminar sem return",
    "variáveis globais não são suportadas",
)
FORWARD_REF_TERSE = "usada antes de ser definida"

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
UI_DIR = os.path.join(ROOT, "tests", "ui")
OK_DIR = os.path.join(ROOT, "tests", "check-ok")


def run(cmd, timeout=20, env=None):
    try:
        p = subprocess.run(cmd, capture_output=True, timeout=timeout, env=env)
        return p.returncode, p.stdout.decode("utf-8", "replace"), p.stderr.decode("utf-8", "replace")
    except subprocess.TimeoutExpired:
        return None, "", "TIMEOUT"


def cdr_files(d):
    return sorted(f for f in os.listdir(d) if f.endswith(".cdr"))


def main():
    if len(sys.argv) != 2:
        print(__doc__)
        sys.exit(2)
    cudrac = sys.argv[1]

    work = tempfile.mkdtemp(prefix="ui_subset_")
    shim_dir = os.path.join(work, "shim")
    os.makedirs(shim_dir, exist_ok=True)
    open(os.path.join(shim_dir, "cudra.py"), "w").write(SHIM)
    env = dict(os.environ, PYTHONPATH=shim_dir)

    fails = []
    counts = {}

    def count(name):
        counts[name] = counts.get(name, 0) + 1

    ok_files = cdr_files(OK_DIR)
    if len(ok_files) < 20:
        fails.append(("POUCOS_CASOS", "check-ok", "", f"há {len(ok_files)}, esperavam-se >=20"))
    for name in ok_files:
        path = os.path.join(OK_DIR, name)
        rc_py, _out, err_py = run(["python3", path] + ARGS, env=env)
        if rc_py != 0:
            fails.append(("CHECK_OK_FALHA_NO_PYTHON", name, "", err_py[-400:]))
            continue
        count("check_ok_py_ok")

        rc_c, _out_c, err_c = run([cudrac, "check", path])
        if rc_c != 0:
            fails.append(("CHECK_OK_REJEITADO_PELO_CUDRAC", name, "", f"rc={rc_c} {err_c[-400:]}"))
            continue
        count("check_ok_cudrac_ok")

    ui_files = cdr_files(UI_DIR)
    if len(ui_files) < 40:
        fails.append(("POUCOS_CASOS", "ui", "", f"há {len(ui_files)}, esperavam-se >=40"))
    for name in ui_files:
        path = os.path.join(UI_DIR, name)
        stderr_path = os.path.join(UI_DIR, name[: -len(".cdr")] + ".stderr")

        rc_c, _out_c, err_c = run([cudrac, "check", path])
        if rc_c != 1:
            fails.append(("UI_NAO_REJEITADO", name, "", f"rc={rc_c} {err_c[-400:]}"))
            continue
        count("ui_cudrac_rejected")

        if not os.path.isfile(stderr_path):
            fails.append(("UI_SEM_STDERR_GRAVADO", name, "", stderr_path))
            continue
        recorded = open(stderr_path, encoding="utf-8").read()

        rc_py, _out_py, err_py = run(["python3", path] + ARGS, env=env)
        py_ok = rc_py == 0
        py_name_error = (not py_ok) and "NameError" in err_py

        if py_ok:
            count("py_ok")
            justified = any(u in recorded for u in UNSUP) or any(
                e in recorded for e in ALLOWED_EXCEPTIONS
            )
            if not justified:
                fails.append(("SEM_AINDA_NAO", name, "", recorded[-400:]))
            else:
                count("py_ok_justificado")
        else:
            count("py_falha_tambem")

        if FORWARD_REF_TERSE in recorded and not py_name_error:
            fails.append(
                (
                    "FORWARD_REF_TERSE_SEM_NAMEERROR",
                    name,
                    "",
                    f"rc_py={rc_py} err_py={err_py[-300:]!r} recorded={recorded[-300:]!r}",
                )
            )
        elif FORWARD_REF_TERSE in recorded:
            count("forward_ref_terse_com_nameerror")

    print("casos:", counts)
    print(f"RESUMO ui_subset_check: {len(fails)} falhas")
    for kind, name, _src, info in fails[:60]:
        print(f"FALHA {kind} [{name}]\n  -> {info}\n")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
