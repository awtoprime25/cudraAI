import io
import os
import random
import subprocess
import sys
import tokenize

KEYWORDS = {
    "True": "True", "False": "False", "None": "NoneKw", "def": "Def", "return": "Return",
    "if": "If", "elif": "Elif", "else": "Else", "while": "While", "for": "For", "in": "In",
    "break": "Break", "continue": "Continue", "pass": "Pass", "and": "And", "or": "Or",
    "not": "Not", "import": "Import", "from": "From",
}
RESERVED = set("class lambda global nonlocal try except finally raise with as yield async await del assert is".split())
OPS = {
    "(": "LParen", ")": "RParen", "[": "LBracket", "]": "RBracket", ":": "Colon", ",": "Comma",
    ".": "Dot", "->": "Arrow", "@": "At", "+": "Plus", "-": "Minus", "*": "Star", "/": "Slash",
    "//": "DoubleSlash", "%": "Percent", "**": "DoubleStar", "&": "Amp", "|": "Pipe", "^": "Caret",
    "~": "Tilde", "<<": "Shl", ">>": "Shr", "<": "Lt", "<=": "Le", ">": "Gt", ">=": "Ge",
    "==": "EqEq", "!=": "NotEq", "=": "Eq", "+=": "PlusEq", "-=": "MinusEq", "*=": "StarEq",
    "/=": "SlashEq", "//=": "DoubleSlashEq", "%=": "PercentEq", "**=": "DoubleStarEq",
    "&=": "AmpEq", "|=": "PipeEq", "^=": "CaretEq", "<<=": "ShlEq", ">>=": "ShrEq",
}


def is_float(s):
    t = s.lower()
    if t.startswith(("0x", "0o", "0b")):
        return False
    return "." in t or "e" in t


def python_tokens(src):
    out = []
    for tok in tokenize.generate_tokens(io.StringIO(src).readline):
        t = tok.type
        if t in (tokenize.NL, tokenize.COMMENT, tokenize.ENCODING):
            continue
        if t == tokenize.NAME:
            k = KEYWORDS.get(tok.string) or ("Reserved" if tok.string in RESERVED else "Name")
            out.append((k, tok.string))
        elif t == tokenize.NUMBER:
            out.append(("Float" if is_float(tok.string) else "Int", tok.string))
        elif t == tokenize.OP:
            out.append((OPS[tok.string], tok.string))
        elif t == tokenize.STRING:
            out.append(("Str", tok.string))
        elif t == tokenize.NEWLINE:
            out.append(("Newline", ""))
        elif t == tokenize.INDENT:
            out.append(("Indent", ""))
        elif t == tokenize.DEDENT:
            out.append(("Dedent", ""))
        elif t == tokenize.ENDMARKER:
            out.append(("Eof", ""))
        else:
            raise RuntimeError(f"token python inesperado {tokenize.tok_name[t]} {tok.string!r}")
    return out


def rnd_number(r):
    return r.choice([
        str(r.randint(0, 10**6)), "0", "00", "1_000", "0x_ff", "0xFF", "0o17", "0b1010", "0b_1",
        "1.5", "1.", ".5", "1e10", "1E-3", "1_0.5e-3", "3.14_15", "9223372036854775807",
        "18446744073709551616", f"{r.random():.6f}",
    ])


NAMES = ["x", "y", "total", "_a", "foo_bar", "i", "n2", "ifx", "define", "notx"]
BINOPS = ["+", "-", "*", "/", "//", "%", "**", "&", "|", "^", "<<", ">>", "<", "<=", ">", ">=", "==", "!=", " and ", " or "]
AUG = ["=", "+=", "-=", "*=", "/=", "//=", "%=", "**=", "&=", "|=", "^=", "<<=", ">>="]


def rnd_expr(r, depth=0):
    c = r.random()
    if depth > 3 or c < 0.3:
        return r.choice([rnd_number(r), r.choice(NAMES), "True", "False", "None"])
    if c < 0.55:
        sp = r.choice(["", " "])
        return f"{rnd_expr(r, depth+1)}{sp}{r.choice(BINOPS)}{sp}{rnd_expr(r, depth+1)}"
    if c < 0.65:
        return r.choice(["-", "+", "~", "not "]) + rnd_expr(r, depth + 1)
    if c < 0.8:
        args = [rnd_expr(r, depth + 1) for _ in range(r.randint(0, 3))]
        sep = r.choice([", ", ",", ",\n        ", " ,\n  # c\n "])
        trail = r.choice(["", ","]) if args else ""
        return f"{r.choice(NAMES)}({sep.join(args)}{trail})"
    if c < 0.9:
        return f"({rnd_expr(r, depth+1)}\n{' ' * r.randint(0, 9)}{r.choice(BINOPS)} {rnd_expr(r, depth+1)})"
    return f"{r.choice(NAMES)}.{r.choice(NAMES)}[{rnd_expr(r, depth+1)}]"


def rnd_block(r, indent, depth):
    lines = []
    for _ in range(r.randint(1, 4)):
        pad = " " * indent
        c = r.random()
        if r.random() < 0.15:
            lines.append(r.choice(["", "   ", "# comentário", pad + "# indentado", "        # fundo"]))
        if depth < 4 and c < 0.35:
            head = r.choice(["if", "while", "elif_chain", "for", "def"])
            step = r.choice([1, 2, 4, 8])
            if head == "for":
                lines.append(f"{pad}for {r.choice(NAMES)} in range({rnd_expr(r)}):")
            elif head == "def":
                lines.append(f"{pad}def {r.choice(NAMES)}(a: int, b: float) -> int:")
            elif head == "elif_chain":
                lines.append(f"{pad}if {rnd_expr(r)}:")
                lines += rnd_block(r, indent + step, depth + 1)
                lines.append(f"{pad}elif {rnd_expr(r)}:")
            else:
                lines.append(f"{pad}{head} {rnd_expr(r)}:")
            lines += rnd_block(r, indent + step, depth + 1)
            if head in ("if", "elif_chain") and r.random() < 0.5:
                lines.append(f"{pad}else:")
                lines += rnd_block(r, indent + step, depth + 1)
        elif c < 0.45:
            lines.append(f"{pad}if {rnd_expr(r)}: {r.choice(NAMES)} = {rnd_expr(r)}")
        elif c < 0.55:
            lines.append(f"{pad}{r.choice(['pass', 'break', 'continue', 'return', 'return ' + rnd_expr(r)])}")
        elif c < 0.62:
            lines.append(f"{pad}{r.choice(NAMES)} = {rnd_expr(r)} \\")
            lines.append(f"{' ' * r.randint(0, 12)}+ {rnd_expr(r)}")
        elif c < 0.66:
            lines.append(f"{pad}from cudra import arg_int, arg_float")
        else:
            lines.append(f"{pad}{r.choice(NAMES)} {r.choice(AUG)} {rnd_expr(r)}  {r.choice(['', '# fim'])}")
    return lines


def rnd_program(r):
    lines = rnd_block(r, 0, 0)
    src = "\n".join(lines)
    nl = r.choice(["\n", "\n", "\r\n"])
    src = src.replace("\n", nl)
    if r.random() < 0.7:
        src += nl
    if r.random() < 0.1:
        src += nl * r.randint(1, 3)
    return src


def cudra_tokens(path, dump):
    p = subprocess.run([dump, path], capture_output=True, text=True, timeout=20)
    toks, errs = [], []
    for line in p.stdout.splitlines():
        parts = line.split("\t")
        if parts[0] == "ERROR":
            errs.append(line)
            continue
        kind, text = parts[0], parts[1] if len(parts) > 1 else ""
        if kind in ("Newline", "Indent", "Dedent", "Eof"):
            text = ""
        toks.append((kind, text))
    return toks, errs, p.returncode, p.stderr


def main():
    dump = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 500
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    workdir = sys.argv[4] if len(sys.argv) > 4 else "/tmp/lexoracle"
    os.makedirs(workdir, exist_ok=True)
    r = random.Random(seed)
    fails = 0
    skipped = 0
    for i in range(n):
        src = rnd_program(r)
        try:
            compile(src, "<gen>", "exec", dont_inherit=True, flags=0x400)
        except SyntaxError:
            pass
        try:
            expected = python_tokens(src)
        except (tokenize.TokenError, IndentationError, SyntaxError):
            skipped += 1
            continue
        path = os.path.join(workdir, f"g{i}.cdr")
        with open(path, "w", newline="") as f:
            f.write(src)
        got, errs, rc, stderr = cudra_tokens(path, dump)
        if rc not in (0, 1) or "panicked" in stderr:
            fails += 1
            print(f"### PANIC/CRASH em {path} rc={rc}\n{stderr[:500]}")
            continue
        if errs:
            fails += 1
            print(f"### ERROS INESPERADOS em {path}: {errs[:3]}")
            continue
        if got != expected:
            fails += 1
            j = next((k for k, (a, b) in enumerate(zip(got, expected)) if a != b), min(len(got), len(expected)))
            print(f"### DIFERENÇA em {path} no token #{j}")
            print("  cudra :", got[max(0, j-3):j+3])
            print("  python:", expected[max(0, j-3):j+3])
            if fails >= 15:
                break
    print(f"\nRESUMO: {n} gerados, {skipped} ignorados (python rejeita), {fails} falhas")


if __name__ == "__main__":
    main()
