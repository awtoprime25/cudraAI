"""Ataques ao lexer: f-strings, entradas patológicas (tempo), UTF-8 inválido,
truncagem em cada posição e sopa de caracteres contra o CPython.

Uso: python3 lexer_attack.py <dump_tokens>   (exit 1 se houver falhas)
"""
import io
import os
import random
import subprocess
import sys
import time
import tokenize
import warnings

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import lexer_oracle as L
import lexer_oracle_v2 as V

warnings.filterwarnings("ignore")
D = sys.argv[1]
SLOW = 2.0
fails = []


def run(src_bytes, name, timeout=20):
    p = f"/tmp/la_{name}.cdr"
    with open(p, "wb") as f:
        f.write(src_bytes)
    t = time.time()
    try:
        r = subprocess.run([D, p], capture_output=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        return None, float(timeout), "", "TIMEOUT"
    return r.returncode, time.time() - t, r.stdout.decode("utf-8", "replace"), r.stderr.decode("utf-8", "replace")


def crashed(rc, err):
    return rc not in (0, 1) or "panicked" in err


def fail(kind, msg):
    fails.append(kind)
    if fails.count(kind) <= 8:
        print(f"FALHA {kind}: {msg}")


print("### 1. f-strings contra o CPython")
FS = [
    "f'{a!r:>10}'", "f'{a!}'", "f'{\"}\"}'", "f'{'{'}'", "f'''{a\n}'''", "f'{x:{y:{z}}}'", "rf'\\{a}'",
    "f'\\{a}'", "f'{#}'", "f'{{'", "f'}}'", "f'{'", "f'{a:{'", "f'{lambda x: 1}'", "f'{a:=1}'", "f'{(a:=1)}'",
    "f'{a!r=}'", "f'{a=!r:>5}'", "f'{ a = }'", "f'{a}' f'{b}'", "f'{a}' 'b'", "f'{\"a\" \"b\"}'",
    "f'{a[\"]\"]}'", "f'{a:}}}'", "f'{a:{{}}}'", "f'{a}}'", "f'{{a}'", "f\"{'''a'''}\"", "f'{f\"{f'{a}'}\"}'",
    "f'''\n{\n a\n # c\n}\n'''", "f'{a:\\n}'", "f'{a!s:{b}}'", "f'{a:%H:%M}'", "f'{\\n}'", "f'{a:{b!r}}'",
    "F'{a}'", "fR'{a}'", "Rf'\\d{a}'", "f'{a:#x}'", "f'{a:,}'", "f'{3.14:.2f}'",
    "f'{a\n}'", "f'''{a\n}'''", "f'{a\\\n}'", "f'{\n}'", "f'{}'", "f'{ }'", "f'{a!x}'", "f'{a!r !s}'",
    "f'\\N{EM DASH}'", "rf'\\N{x}'", "f'\\}}'", "f'\\\\{a}'", "f'\\{{'", "f'\\N{EM DASH}{a}'", "f'\\N{'",
    "f'\\N{EM DASH'", "fr'\\N{BULLET}'", "f'\\n{a}'", "f'\\'{a}'", "f'{a}\\''", "f'\\\\N{a}'", "f'\\x{a}'",
    "f'{a:\\N{BULLET}}'", "f'\\N{EM DASH}}'", "f'\\N{EM DASH}' + 1", "'\\N{x}'", "b'\\N{x}'",
]
for i, s in enumerate(FS):
    src = "x = " + s + "\n"
    try:
        list(tokenize.generate_tokens(io.StringIO(src).readline))
        tk_ok = True
    except Exception:
        tk_ok = False
    try:
        compile(src, "t", "exec")
        cp = None
    except SyntaxError as e:
        cp = e.msg
    rc, dt, out, err = run(src.encode(), f"fs{i}")
    errs = [ln.split("\t")[1] for ln in out.split("\n") if ln.startswith("ERROR")]
    if crashed(rc, err):
        fail("CRASH_FSTRING", f"{s!r} {err[:200]}")
    elif cp is None and errs:
        fail("FSTRING_NOS_REJEITAMOS_PY_ACEITA", f"{s!r} -> {errs[:2]}")
    elif not tk_ok and not errs:
        fail("FSTRING_TOKENIZE_REJEITA_NOS_ACEITAMOS", f"{s!r} (compile: {cp})")

print("### 2. patológicos (tempo)")
PATH = {
    "1MB_parens": b"x = " + b"(" * 1_000_000 + b"\n",
    "1MB_closers": b"x = " + b")" * 1_000_000 + b"\n",
    "1MB_line": b"x = " + b"a+" * 500_000 + b"a\n",
    "1MB_str": b"x = '" + b"a" * 1_000_000 + b"'\n",
    "unclosed_str_lines": b"'\n" * 300_000,
    "unclosed_triple": b"'''" + b"a\n" * 300_000,
    "bs_cont_300k": b"x = 1 \\\n" * 300_000 + b"+ 1\n",
    "ws_bs_lines": b"  \\\n" * 300_000 + b"x = 1\n",
    "fstring_100k_fields": b"x = f'" + b"{a}" * 100_000 + b"'\n",
    "fstring_nested_200": b"x = " + b"f'{" * 200 + b"a" + b"}'" * 200 + b"\n",
    "fstring_unclosed_fields": b"x = f'" + b"{" * 100_000 + b"\n",
    "fstring_bs_brace": b"x = f'" + b"\\{a}" * 200_000 + b"'\n",
    "fstring_named_esc": b"x = f'" + b"\\N{BULLET}" * 100_000 + b"'\n",
    "indent_zigzag": b"if a:\n" + b"  x = 1\n x = 2\n" * 100_000,
    "many_errors": b"$" * 1_000_000 + b"\n",
    "nul_bytes": b"\x00" * 1_000_000,
    "comments_300k": b"# c\n" * 300_000,
    "crlf_300k": b"x = 1\r\n" * 300_000,
    "cr_only_300k": b"x = 1\r" * 300_000,
    "long_ident": b"x" * 2_000_000 + b" = 1\n",
    "long_number": b"x = " + b"1" * 2_000_000 + b"\n",
    "long_float": b"x = 1." + b"1" * 2_000_000 + b"e5\n",
    "underscores": b"x = 1" + b"_1" * 1_000_000 + b"\n",
    "mismatch_storm": b"x = " + b"(]" * 300_000 + b"\n",
    "ellipsis_storm": b"x = " + b"." * 1_000_000 + b"\n",
    "unicode_line_tokens": ("x = " + "é+" * 300_000 + "\n").encode(),
    "unicode_then_tokens": ("x = '" + "é" * 300_000 + "' + " + "a+" * 300_000 + "a\n").encode(),
    "emoji_comment": ("# " + "🙂" * 500_000 + "\nx = 1\n").encode(),
}
for name, b in PATH.items():
    rc, dt, out, err = run(b, name)
    nerr = sum(1 for ln in out.split("\n") if ln.startswith("ERROR"))
    if crashed(rc, err):
        fail("CRASH_PATOLOGICO", f"{name} rc={rc} {err[:150]!r}")
    elif dt > SLOW:
        fail("LENTO", f"{name} {dt:.2f}s")
    print(f"   {name:24} rc={rc} {dt:6.2f}s erros={nerr}")

print("### 3. UTF-8 inválido e bytes estranhos")
for name, b in {"bad_utf8": b"x = 1\n\xff\xfe\n", "lone_cont": b"x = \x80\n", "truncated_utf8": b"x = '\xc3",
                "bom_only": b"\xef\xbb\xbf", "bom_twice": b"\xef\xbb\xbf\xef\xbb\xbfx = 1\n",
                "overlong": b"x = \xc0\xaf\n", "nul_in_comment": b"x = 1  # a\x00b\n",
                "nul_in_string": b"x = 'a\x00b'\n"}.items():
    rc, dt, out, err = run(b, name)
    nerr = sum(1 for ln in out.split("\n") if ln.startswith("ERROR"))
    if crashed(rc, err):
        fail("CRASH_BYTES", f"{name} {err[:150]!r}")
    elif name.startswith("nul") and nerr != 1:
        fail("NUL", f"{name}: esperado exatamente 1 erro, obtido {nerr}: {out[-200:]!r}")
    print(f"   {name:16} rc={rc} erros={nerr}")

print("### 4. truncagem em cada posição de char")
r = random.Random(7)
progs = [L.rnd_program(r) for _ in range(6)] + [V.mutate_program(r, L.rnd_program(r)) for _ in range(6)]
progs += ["x = f'{d['k']}' + f'''{a\n:>{w}}'''\nif a:\n    y = (1,\n  2]\n",
          "def f(a: int) -> int:\n    return a \\\n  + 1\n", FS[4] + FS[29], "x = f'\\N{EM DASH}\\{a}' + rf'\\N{b}'\n"]
total = 0
for pi, src in enumerate(progs):
    for k in range(len(src) + 1):
        total += 1
        rc, dt, out, err = run(src[:k].encode(), "trunc")
        if crashed(rc, err) or dt > 5:
            rc, dt, out, err = run(src[:k].encode(), "trunc_retry")
            if crashed(rc, err) or dt > 5:
                fail("TRUNCAGEM", f"prog {pi} @ {k}: {src[:k][-60:]!r} rc={rc} {dt:.2f}s {err[:150]}")
print(f"   truncagens: {total}")

print("### 5. sopa de caracteres vs CPython")
ALPH = list("()[]{}:;,.\\'\"#\n\r\t \x0c\x0b\x00=+-*/%<>!&|^~@0123456789abcdefxyzjJeE_rbfuRBFUN") + \
    ["é", "\u2028", "\u00a0", "\ufeff", "🙂", "·", "\u0300"]
cats = {}
for i in range(3000):
    src = "".join(r.choice(ALPH) for _ in range(r.randint(1, 30)))
    rc, dt, out, err = run(src.encode("utf-8"), "soup")
    if crashed(rc, err):
        fail("CRASH_SOPA", f"{src!r} {err[:150]}")
        continue
    errs = [ln for ln in out.split("\n") if ln.startswith("ERROR")]
    py = src.replace("\r\n", "\n").replace("\r", "\n")
    try:
        compile(py, "t", "exec")
        cp = None
    except (SyntaxError, ValueError) as e:
        cp = getattr(e, "msg", str(e))
    key = ("py_ok" if cp is None else "py_err") + "/" + ("nos_ok" if not errs else "nos_err")
    cats[key] = cats.get(key, 0) + 1
    if cp is not None and not errs:
        # um U+FEFF no início é o BOM de um ficheiro; o compile() de uma str não o trata assim.
        bom_artifact = src.startswith("\ufeff") and "U+FEFF" in cp
        lexical = any(m in cp for m in V.LEXICAL_COMPILE_MSGS) or "non-printable" in cp or "null bytes" in cp
        if lexical and not bom_artifact:
            fail("SOPA_LEXICO_ACEITE", f"{src!r} | {cp}")
print("   sopa:", cats)

print(f"RESUMO ataque: {len(fails)} falhas {dict((k, fails.count(k)) for k in sorted(set(fails)))}")
sys.exit(1 if fails else 0)
