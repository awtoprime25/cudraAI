import io
import os
import random
import subprocess
import sys
import tokenize
from collections import Counter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import lexer_oracle as L

INTENTIONAL = [
    "tabs", "complex", "falta um espaço", "não-ASCII", "zeros à esquerda",
    "carácter inesperado",
]

# Erros que o compile() dá ao nível do tokenizer C mas que o módulo tokenize
# não reporta: se o compile() rejeita com um destes e nós aceitamos, é falha.
LEXICAL_COMPILE_MSGS = [
    "closing parenthesis", "unmatched", "too many nested parentheses",
    "too many levels of indentation", "invalid digit", "invalid decimal literal",
    "invalid hexadecimal literal", "invalid octal literal", "invalid binary literal",
    "leading zeros", "unterminated", "invalid non-printable character",
    "unindent does not match", "unexpected EOF", "was never closed", "invalid character",
]


class LegacyDiamond(Exception):
    pass


def unescape(s):
    out, i = [], 0
    while i < len(s):
        c = s[i]
        if c == "\\" and i + 1 < len(s):
            n = s[i + 1]
            out.append({"\\": "\\", "t": "\t", "n": "\n", "r": "\r"}.get(n, "\\" + n))
            i += 2
        else:
            out.append(c)
            i += 1
    return "".join(out)


def py_tokens(src):
    toks = []
    fdepth = 0
    fstart = None
    for tok in tokenize.generate_tokens(io.StringIO(src).readline):
        t = tok.type
        name = tokenize.tok_name[t]
        if name == "FSTRING_START":
            if fdepth == 0:
                fstart = tok.start
            fdepth += 1
            continue
        if name == "FSTRING_END":
            fdepth -= 1
            if fdepth == 0:
                toks.append(("Str", None, fstart, tok.end))
            continue
        if fdepth:
            continue
        if t == tokenize.ERRORTOKEN:
            raise SyntaxError(f"ERRORTOKEN {tok.string!r}")
        if t in (tokenize.NL, tokenize.COMMENT, tokenize.ENCODING):
            continue
        if t == tokenize.NAME:
            k = L.KEYWORDS.get(tok.string) or ("Reserved" if tok.string in L.RESERVED else "Name")
        elif t == tokenize.NUMBER:
            k = "Float" if L.is_float(tok.string) else "Int"
        elif t == tokenize.OP:
            if tok.string == "<>":
                raise LegacyDiamond()
            if tok.string not in L.OPS:
                raise SyntaxError(f"op desconhecido {tok.string!r}")
            k = L.OPS[tok.string]
        elif t == tokenize.STRING:
            k = "Str"
        elif t == tokenize.NEWLINE:
            k = "Newline"
        elif t == tokenize.INDENT:
            k = "Indent"
        elif t == tokenize.DEDENT:
            k = "Dedent"
        elif t == tokenize.ENDMARKER:
            k = "Eof"
        else:
            raise RuntimeError(name)
        toks.append((k, tok.string, tok.start, tok.end))
    return toks


def slice_text(src_lines, start, end):
    (r1, c1), (r2, c2) = start, end
    if r1 == r2:
        return src_lines[r1 - 1][c1:c2]
    parts = [src_lines[r1 - 1][c1:]] + src_lines[r1:r2 - 1] + [src_lines[r2 - 1][:c2]]
    return "".join(parts)


def exp_s_none(t):
    return t.lstrip("rRbBuU").startswith(("f", "F")) or t[:2].lower() in ("rf", "fr")


def run_cudra(dump, path):
    p = subprocess.run([dump, path], capture_output=True, timeout=20)
    stdout = p.stdout.decode("utf-8", "replace")
    stderr = p.stderr.decode("utf-8", "replace")
    toks, errs = [], []
    for line in stdout.split("\n"):
        if not line:
            continue
        parts = line.split("\t")
        if parts[0] == "ERROR":
            errs.append(parts[1] if len(parts) > 1 else line)
            continue
        kind = parts[0]
        text = unescape(parts[1]) if len(parts) > 1 else ""
        lc = parts[2] if len(parts) > 2 else "0:0"
        ln, col = (int(x) for x in lc.split(":"))
        toks.append((kind, text, (ln, col - 1)))
    return toks, errs, p.returncode, stderr


STRS = [
    "'abc'", '"x y"', "'''a\nb'''", '"""multi\n  linha\n"""', "r'\\d+'", "b'\\x00'", "f'fixo'",
    "'esc\\'aped'", '"tab\tdentro"', "'café 🙂'", "Rb'raw'", "u'uni'", "''", '""', "'''\n'''",
    "'a' 'b'", "'\\\n'",
    "f'{d['k']}'", "f'{a +\n b}'", "f'{a # c\n}'", "f'{a:>{w}}'", "f'{{a}}'",
    "f\"{'x' if a else \"y\"}\"", "f'''{'''a'''}'''", "f'{f'{a}'}'", "rf'\\d{a}'",
    "f'{a!r}'", "f'{a=}'", "f'{a}}}'", "F'{(a,\n b)}'", "f'x{a}y{b:{c}.{d}}z'",
    "ub'a'", "ur'a'", "ff'a'", "bf'a'", "uR'a'", "Br'a'", "fR'a'", "rB'a'",
]
COMMENTS = ["# café é bom", "# 🙂 emoji", "#\ttab no comentário", "# \\ barra", "#", "# ''' não é string"]


def mutate_program(r, src):
    lines = src.split("\n")
    for _ in range(r.randint(1, 4)):
        i = r.randrange(len(lines))
        c = r.random()
        if c < 0.25:
            lines[i] = lines[i] + "  " + r.choice(COMMENTS)
        elif c < 0.45:
            lines[i] = lines[i].rstrip("\\ ") + " + " + r.choice(STRS) if lines[i].strip() and not lines[i].rstrip().endswith(":") else lines[i]
        elif c < 0.55:
            lines.insert(i, r.choice(["", "   ", "\x0c", "\x0c    x = 1", "  \t  ", "# só comentário"]))
        elif c < 0.62:
            lines[i] = lines[i] + r.choice(["   ", "\t", " \\", ""])
        else:
            lines.insert(i, r.choice(["x = " + r.choice(STRS), r.choice(STRS)]))
    return "\n".join(lines)


def char_fuzz(r, src):
    s = list(src)
    for _ in range(r.randint(1, 3)):
        op = r.random()
        pos = r.randrange(len(s) + 1)
        if op < 0.4 and s:
            del s[min(pos, len(s) - 1)]
        elif op < 0.8:
            s.insert(pos, r.choice(list("()[]{};:\\'\"#\n \t.0123456789eExXjJ_+-*/=<>!$?`@~é")))
        else:
            s.insert(pos, r.choice(["\"\"\"", "'''", "\\\n", "\n    ", "0x", "1e", "0o8", "0b2", "1_", "._",
                                    ":=", "@=", "...", "f'{", "0x1g", "0b1a", "0o19"]))
    return "".join(s)


def main():
    dump = sys.argv[1]
    n = int(sys.argv[2])
    seed = int(sys.argv[3])
    mode = sys.argv[4]
    work = f"/tmp/or2_{mode}_{seed}"
    os.makedirs(work, exist_ok=True)
    r = random.Random(seed)
    cats = Counter()
    shown = Counter()
    for i in range(n):
        base = L.rnd_program(r)
        src = mutate_program(r, base) if mode == "rich" else char_fuzz(r, base)
        path = os.path.join(work, f"c{i}.cdr")
        with open(path, "w", newline="", encoding="utf-8") as f:
            f.write(src)
        py_src = src.replace("\r\n", "\n").replace("\r", "\n")
        py_err = None
        try:
            exp = py_tokens(py_src)
        except LegacyDiamond:
            cats["legado_py2_diamante"] += 1
            continue
        except (tokenize.TokenError, SyntaxError, IndentationError) as e:
            py_err = f"{type(e).__name__}: {e}"
        compile_err = None
        try:
            compile(py_src, "<t>", "exec", dont_inherit=True)
        except SyntaxError as e:
            compile_err = e.msg
        except Exception as e:
            compile_err = repr(e)
        got, errs, rc, stderr = run_cudra(dump, path)
        cat = None
        detail = ""
        if rc not in (0, 1) or "panicked" in stderr:
            cat, detail = "CRASH", stderr[:300]
        elif py_err and not errs:
            cat, detail = "PY_REJEITA_NOS_ACEITAMOS", py_err
        elif not py_err and errs:
            if any(any(k in e for k in INTENTIONAL) for e in errs):
                cats["divergencia_intencional"] += 1
                continue
            if compile_err:
                cats["tokenize_aceita_compile_rejeita"] += 1
                if shown["compile_rej"] < 3:
                    shown["compile_rej"] += 1
                    print(f"--- (rever) nós: {errs[:1]} | compile: {compile_err} | {path}")
                continue
            cat, detail = "NOS_REJEITAMOS_PY_ACEITA", "; ".join(errs[:2])
        elif not py_err:
            exp_cmp = []
            for k, s, start, end in exp:
                if k in ("Newline", "Indent", "Dedent", "Eof"):
                    exp_cmp.append((k, None, None))
                else:
                    exp_cmp.append((k, s, start))
            got_cmp = [(k, None, None) if k in ("Newline", "Indent", "Dedent", "Eof") else (k, None if (k == "Str" and exp_s_none(t)) else t, p) for k, t, p in got]
            exp_cmp = [(k, None if (k == "Str" and s is None) else s, p) for k, s, p in exp_cmp]
            if got_cmp != exp_cmp:
                j = next((x for x, (a, b) in enumerate(zip(got_cmp, exp_cmp)) if a != b), min(len(got_cmp), len(exp_cmp)))
                cat = "DIFERENCA_TOKENS"
                detail = f"#{j}\n    cudra : {got_cmp[max(0,j-2):j+3]}\n    python: {exp_cmp[max(0,j-2):j+3]}"
            elif compile_err and any(m in compile_err for m in LEXICAL_COMPILE_MSGS):
                cat, detail = "COMPILE_REJEITA_LEXICO_NOS_ACEITAMOS", compile_err
            else:
                cats["ok"] += 1
                continue
        else:
            cats["ambos_rejeitam"] += 1
            continue
        cats[cat] += 1
        if shown[cat] < 4:
            shown[cat] += 1
            print(f"### {cat} {path}\n  src={src[:300]!r}\n  {detail}")
    print(f"RESUMO {mode} seed={seed}: {dict(cats)}")


if __name__ == "__main__":
    main()
