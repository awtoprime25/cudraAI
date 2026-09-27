"""Oráculo diferencial do parser: compara `dump_ast` com o ast.parse do CPython 3.12.

Uso: python3 parser_oracle.py <dump_ast> <N> <seed> <valid|wild|fuzz|deep>

Formato S-expr (uma linha por item de topo, um espaço entre elementos):
  item   := stmt | (def NOME (decos E*) (params P*) TY|_ (body S*))
  P      := (p NOME TY|_)
  TY     := (ty NOME) | (ty NOME TY+) | (ty None)      Array[f32] -> (ty Array (ty f32))
  S      := (assign T TY|_ E) | (aug OP T E) | (for NOME E (body S*)) | (while E (body S*))
          | (if E (body S*)) | (if E (body S*) (else S*))   elif -> else com um único if
          | (return E) | (return _) | (break) | (continue) | (pass) | (expr E)
          | (import NOME) | (from NOME NOME+)
  T      := (name x) | (index E E)
  E      := (int N) | (float HEX16) | (bool True) | (bool False) | (none) | (name x)
          | (unary neg|pos|not|invert E) | (bin OP E E) | (and E E+) | (or E E+)
          | (cmp E (OPC E)+ ) escrito como (cmp E lt E le E) | (call E E*) | (attr E nome) | (index E E)
  OP     := add sub mul div floordiv mod pow bitand bitor bitxor shl shr
  OPC    := eq ne lt le gt ge
  and/or: achatados (um and dentro de and, em qualquer posição, junta-se à lista).
  float: bits IEEE-754 big-endian em hex, 16 dígitos minúsculos.
Erros: linhas `ERROR\t<mensagem>\t<linha>:<coluna>`; exit 0 sem erros, 1 com erros.
"""
import ast
import os
import random
import struct
import subprocess
import sys
import threading
import tokenize
import io
import warnings
from collections import Counter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import lexer_oracle as L
import lexer_oracle_v2 as V

warnings.filterwarnings("ignore")

INTENTIONAL = [
    "ainda não é suportad", "ainda não são suportad", "demasiado aninhad", "tabs", "complex", "falta um espaço",
    "não-ASCII", "zeros à esquerda", "carácter inesperado",
]
MAX_DEPTH = 1000
STATUS = []


class Unsupported(Exception):
    pass


BIN = {ast.Add: "add", ast.Sub: "sub", ast.Mult: "mul", ast.Div: "div", ast.FloorDiv: "floordiv",
       ast.Mod: "mod", ast.Pow: "pow", ast.BitAnd: "bitand", ast.BitOr: "bitor",
       ast.BitXor: "bitxor", ast.LShift: "shl", ast.RShift: "shr"}
UN = {ast.USub: "neg", ast.UAdd: "pos", ast.Not: "not", ast.Invert: "invert"}
CMP = {ast.Eq: "eq", ast.NotEq: "ne", ast.Lt: "lt", ast.LtE: "le", ast.Gt: "gt", ast.GtE: "ge"}


def fhex(x):
    return struct.pack(">d", x).hex()


def ex(e):
    t = type(e)
    if t is ast.Constant:
        v = e.value
        if isinstance(v, bool):
            return f"(bool {v})"
        if v is None:
            return "(none)"
        if isinstance(v, int):
            if v > 2**64 - 1:
                raise Unsupported("int > u64")
            return f"(int {v})"
        if isinstance(v, float):
            return f"(float {fhex(v)})"
        raise Unsupported(type(v).__name__)
    if t is ast.Name:
        return f"(name {e.id})"
    if t is ast.UnaryOp:
        return f"(unary {UN[type(e.op)]} {ex(e.operand)})"
    if t is ast.BinOp:
        if type(e.op) not in BIN:
            raise Unsupported("matmul")
        return f"(bin {BIN[type(e.op)]} {ex(e.left)} {ex(e.right)})"
    if t is ast.BoolOp:
        name = "and" if isinstance(e.op, ast.And) else "or"
        return f"({name} {' '.join(flat_bool(e, type(e.op)))})"
    if t is ast.Compare:
        parts = [ex(e.left)]
        for op, c in zip(e.ops, e.comparators):
            if type(op) not in CMP:
                raise Unsupported("is/in")
            parts += [CMP[type(op)], ex(c)]
        return f"(cmp {' '.join(parts)})"
    if t is ast.Call:
        if e.keywords or any(isinstance(a, ast.Starred) for a in e.args):
            raise Unsupported("kwargs/star")
        return f"(call {' '.join([ex(e.func)] + [ex(a) for a in e.args])})"
    if t is ast.Attribute:
        return f"(attr {ex(e.value)} {e.attr})"
    if t is ast.Subscript:
        if isinstance(e.slice, (ast.Slice, ast.Tuple)):
            raise Unsupported("slice/tuple index")
        return f"(index {ex(e.value)} {ex(e.slice)})"
    raise Unsupported(t.__name__)


def flat_bool(e, op):
    out = []
    for v in e.values:
        if isinstance(v, ast.BoolOp) and type(v.op) is op:
            out += flat_bool(v, op)
        else:
            out.append(ex(v))
    return out


def ty(a):
    if isinstance(a, ast.Name):
        return f"(ty {a.id})"
    if isinstance(a, ast.Constant) and a.value is None:
        return "(ty None)"
    if isinstance(a, ast.Subscript) and isinstance(a.value, ast.Name):
        args = a.slice.elts if isinstance(a.slice, ast.Tuple) else [a.slice]
        return f"(ty {a.value.id} {' '.join(ty(x) for x in args)})"
    raise Unsupported("tipo")


def target(t):
    if isinstance(t, ast.Name):
        return f"(name {t.id})"
    if isinstance(t, ast.Subscript) and not isinstance(t.slice, (ast.Slice, ast.Tuple)):
        return f"(index {ex(t.value)} {ex(t.slice)})"
    raise Unsupported("alvo")


def body(stmts):
    return " ".join(st(s, False) for s in stmts)


def st(s, top):
    t = type(s)
    if t is ast.Assign:
        if len(s.targets) != 1:
            raise Unsupported("atribuição múltipla")
        return f"(assign {target(s.targets[0])} _ {ex(s.value)})"
    if t is ast.AnnAssign:
        if s.value is None or not isinstance(s.target, ast.Name):
            raise Unsupported("anotação")
        return f"(assign {target(s.target)} {ty(s.annotation)} {ex(s.value)})"
    if t is ast.AugAssign:
        if type(s.op) not in BIN:
            raise Unsupported("@=")
        return f"(aug {BIN[type(s.op)]} {target(s.target)} {ex(s.value)})"
    if t is ast.For:
        if not isinstance(s.target, ast.Name) or s.orelse:
            raise Unsupported("for")
        return f"(for {s.target.id} {ex(s.iter)} (body {body(s.body)}))".replace(" )", ")")
    if t is ast.While:
        if s.orelse:
            raise Unsupported("while else")
        return f"(while {ex(s.test)} (body {body(s.body)}))".replace(" )", ")")
    if t is ast.If:
        head = f"(if {ex(s.test)} (body {body(s.body)})"
        if s.orelse:
            head += f" (else {body(s.orelse)})"
        return (head + ")").replace(" )", ")")
    if t is ast.Return:
        return f"(return {ex(s.value)})" if s.value is not None else "(return _)"
    if t is ast.Break:
        return "(break)"
    if t is ast.Continue:
        return "(continue)"
    if t is ast.Pass:
        return "(pass)"
    if t is ast.Expr:
        return f"(expr {ex(s.value)})"
    if t is ast.Import:
        if len(s.names) != 1 or s.names[0].asname or "." in s.names[0].name:
            raise Unsupported("import")
        return f"(import {s.names[0].name})"
    if t is ast.ImportFrom:
        if s.level or not s.module or "." in s.module or any(n.asname or n.name == "*" for n in s.names):
            raise Unsupported("from import")
        return f"(from {s.module} {' '.join(n.name for n in s.names)})"
    if t is ast.FunctionDef:
        a = s.args
        if not top or a.posonlyargs or a.vararg or a.kwonlyargs or a.kwarg or a.defaults or getattr(s, "type_params", None):
            raise Unsupported("def")
        decos = " ".join(ex(d) for d in s.decorator_list)
        params = " ".join(f"(p {p.arg} {ty(p.annotation) if p.annotation else '_'})" for p in a.args)
        ret = ty(s.returns) if s.returns else "_"
        return (f"(def {s.name} (decos {decos}) (params {params}) {ret} (body {body(s.body)}))").replace(" )", ")")
    raise Unsupported(t.__name__)


def module_sexpr(m):
    return [st(s, True) for s in m.body]


# ---- profundidade da AST de expressões, com a definição do nosso parser ----
# Contam só as construções que o parser aninha mesmo por recursão: `-x`,
# `not x`, `x ** y` (associativo à direita), parênteses, chamadas, atributos,
# índices e cada comparação em cadeia. As cadeias associativas à esquerda que
# o parser constrói em ciclo — os operadores binários exceto `**`, tal como
# `and`/`or` — não contam para o limite de aninhamento: não têm limite de
# tamanho e são aceites com centenas de milhares de termos.
def _expr_kids(e):
    if type(e) is ast.BoolOp:
        return list(e.values)
    return [c for c in ast.iter_child_nodes(e) if isinstance(c, ast.expr)]


def _left_chain(e):
    return type(e) is ast.BoolOp or (type(e) is ast.BinOp and not isinstance(e.op, ast.Pow))


def depth(root):
    res = {}
    stack = [(root, False)]
    while stack:
        e, done = stack.pop()
        kids = _expr_kids(e)
        if not done:
            stack.append((e, True))
            stack.extend((c, False) for c in kids)
            continue
        ds = [res[id(c)] for c in kids]
        if _left_chain(e):
            d = max(ds, default=0)
        else:
            d = 1 + max(ds, default=0)
        res[id(e)] = d
    return res[id(root)]


def max_depth_module(m):
    best = 0
    for node in ast.walk(m):
        if isinstance(node, ast.stmt):
            for f in ("value", "test", "iter", "target"):
                v = getattr(node, f, None)
                if isinstance(v, ast.expr):
                    best = max(best, depth(v))
            if isinstance(node, ast.Assign):
                for tg in node.targets:
                    best = max(best, depth(tg))
            if isinstance(node, ast.FunctionDef):
                for d in node.decorator_list:
                    best = max(best, depth(d))
    return best


# ---- gerador de programas válidos no subconjunto ----
PREC = {"or": 1, "and": 2, "not": 3, "cmp": 4, "|": 5, "^": 6, "&": 7, "<<": 8, ">>": 8,
        "+": 9, "-": 9, "*": 10, "/": 10, "//": 10, "%": 10, "u": 11, "**": 12, "post": 14, "atom": 15}
BINS = ["|", "^", "&", "<<", ">>", "+", "-", "*", "/", "//", "%", "**"]
CMPS = ["==", "!=", "<", "<=", ">", ">="]
NAMES = ["x", "y", "total", "_a", "foo_bar", "i", "n2", "match", "case", "type", "Array", "cudra"]
NUMS = ["0", "7", "00", "1_000", "0x_ff", "0XAB", "0o17", "0b1010", "0B_1", "18446744073709551615",
        "9223372036854775808", "1.5", "1.", ".5", "1e10", "1E-3", "1_0.5e-3", "3.14_15", "1e400", "0.1",
        "123456789012345678.0", "2.5e-05", "0e0"]


class G:
    def __init__(self, r):
        self.r = r

    def ws(self):
        return self.r.choice(["", " ", " ", "  "])

    def wrap(self, s, need):
        r = self.r
        if need or r.random() < 0.07:
            if r.random() < 0.2:
                return f"({r.choice([' ', chr(10) + '    ', ' # c' + chr(10) + '  '])}{s}{r.choice(['', ' ', chr(10)])})"
            return f"({s})"
        return s

    def expr(self, d=0, allow_bool=True):
        """Devolve (texto, precedência)."""
        r = self.r
        c = r.random()
        if d > 4 or c < 0.25:
            k = r.random()
            if k < 0.45:
                return r.choice(NUMS), PREC["atom"]
            if k < 0.85:
                return r.choice(NAMES), PREC["atom"]
            return r.choice(["True", "False", "None"]), PREC["atom"]
        if c < 0.5:
            op = r.choice(BINS)
            p = PREC[op]
            a, pa = self.expr(d + 1)
            b, pb = self.expr(d + 1)
            if op == "**":
                a = self.wrap(a, pa <= p or pa == PREC["u"])
                b = self.wrap(b, pb < PREC["u"])
            else:
                a = self.wrap(a, pa < p)
                b = self.wrap(b, pb <= p)
            return f"{a}{self.ws()}{op}{self.ws()}{b}", p
        if c < 0.6:
            op = r.choice(["-", "+", "~"])
            a, pa = self.expr(d + 1)
            a = self.wrap(a, pa < PREC["u"])
            return f"{op}{self.ws()}{a}", PREC["u"]
        if c < 0.66 and allow_bool:
            a, pa = self.expr(d + 1)
            a = self.wrap(a, pa < PREC["not"])
            return f"not {a}", PREC["not"]
        if c < 0.74:
            n = r.randint(1, 3)
            parts = []
            for _ in range(n + 1):
                a, pa = self.expr(d + 1)
                parts.append(self.wrap(a, pa <= PREC["cmp"]))
            out = parts[0]
            for x in parts[1:]:
                out += f" {r.choice(CMPS)} {x}"
            return out, PREC["cmp"]
        if c < 0.82 and allow_bool:
            op = r.choice(["and", "or"])
            p = PREC[op]
            n = r.randint(2, 4)
            parts = []
            for _ in range(n):
                a, pa = self.expr(d + 1)
                parts.append(self.wrap(a, pa <= p))
            return f" {op} ".join(parts), p
        # postfix
        base, pb = self.expr(d + 2)
        if base[0].isdigit() or base[0] == ".":
            base = f"({base})"
        else:
            base = self.wrap(base, pb < PREC["post"])
        k = r.random()
        if k < 0.5:
            args = [self.expr(d + 1)[0] for _ in range(r.randint(0, 3))]
            sep = r.choice([", ", ",", ",\n        ", " ,\n  # c\n "])
            trail = r.choice(["", ","]) if args else ""
            return f"{base}({sep.join(args)}{trail})", PREC["post"]
        if k < 0.75:
            return f"{base}.{r.choice(NAMES)}", PREC["post"]
        return f"{base}[{self.expr(d + 1)[0]}]", PREC["post"]

    def e(self):
        return self.expr()[0]

    def ty(self, d=0):
        r = self.r
        if d < 2 and r.random() < 0.25:
            args = ", ".join(self.ty(d + 1) for _ in range(r.randint(1, 2)))
            return f"Array[{args}]"
        return r.choice(["int", "float", "bool", "f32", "i32"])

    def simple(self, in_def, in_loop):
        r = self.r
        c = r.random()
        n = r.choice(NAMES)
        if c < 0.25:
            tgt = r.choice([n, n, f"({n})", f"{n}[{self.e()}]"])
            return f"{tgt} = {self.e()}"
        if c < 0.35:
            return f"{n}: {self.ty()} = {self.e()}"
        if c < 0.5:
            tgt = r.choice([n, n, f"({n})", f"{n}[{self.e()}]"])
            return f"{tgt} {r.choice(L.AUG[1:])} {self.e()}"
        if c < 0.62:
            return f"{r.choice(NAMES + ['print'])}({', '.join(self.e() for _ in range(r.randint(0, 3)))})"
        if c < 0.7 and in_def:
            return r.choice(["return", f"return {self.e()}"])
        if c < 0.78 and in_loop:
            return r.choice(["break", "continue"])
        if c < 0.85:
            return "pass"
        return f"{self.e()}"

    def simple_line(self, in_def, in_loop):
        r = self.r
        k = r.randint(1, 3) if r.random() < 0.25 else 1
        s = "; ".join(self.simple(in_def, in_loop) for _ in range(k))
        if r.random() < 0.1:
            s += ";"
        if r.random() < 0.1:
            s += "  # fim"
        return s

    def block(self, ind, depth_, in_def, in_loop):
        r = self.r
        lines = []
        for _ in range(r.randint(1, 4)):
            pad = " " * ind
            if r.random() < 0.1:
                lines.append(r.choice(["", "   ", "# comentário", pad + "# c"]))
            c = r.random()
            step = r.choice([1, 2, 4, 4, 8])
            if depth_ < 4 and c < 0.35:
                kind = r.choice(["if", "while", "for", "if"])
                loop = in_loop or kind in ("while", "for")
                if kind == "for":
                    v = r.choice(NAMES)
                    v = f"({v})" if r.random() < 0.1 else v
                    lines.append(f"{pad}for {v} in {self.e()}:")
                else:
                    lines.append(f"{pad}{kind} {self.e()}:")
                if r.random() < 0.2:
                    lines[-1] += " " + self.simple_line(in_def, loop)
                else:
                    lines += self.block(ind + step, depth_ + 1, in_def, loop)
                if kind == "if":
                    for _ in range(r.choice([0, 0, 1, 2])):
                        lines.append(f"{pad}elif {self.e()}:")
                        lines += self.block(ind + step, depth_ + 1, in_def, in_loop)
                    if r.random() < 0.4:
                        lines.append(f"{pad}else:")
                        lines += self.block(ind + step, depth_ + 1, in_def, in_loop)
            elif c < 0.42:
                lines.append(f"{pad}{r.choice(NAMES)} = {self.e()} \\")
                b, pb = self.expr()
                lines.append(f"{' ' * r.randint(0, 12)}+ {self.wrap(b, pb <= PREC['+'])}")
            else:
                lines.append(pad + self.simple_line(in_def, in_loop))
        return lines

    def program(self):
        r = self.r
        lines = []
        if r.random() < 0.4:
            lines.append(r.choice(["from cudra import arg_int, arg_float", "import cudra",
                                   "from cudra import (arg_int,\n    arg_float,)", "from cudra import arg_int"]))
        for _ in range(r.randint(1, 4)):
            if r.random() < 0.4:
                for _ in range(r.choice([0, 0, 1, 2])):
                    lines.append("@" + r.choice(["gpu.kernel", "d", "a.b(1, x)", "f()"]))
                params = []
                for _ in range(r.randint(0, 3)):
                    p = r.choice(NAMES)
                    params.append(f"{p}: {self.ty()}" if r.random() < 0.8 else p)
                trail = "," if params and r.random() < 0.2 else ""
                ret = f" -> {r.choice([self.ty(), 'None'])}" if r.random() < 0.6 else ""
                lines.append(f"def {r.choice(NAMES)}({', '.join(params)}{trail}){ret}:")
                lines += self.block(r.choice([1, 2, 4]), 1, True, False)
            else:
                lines += self.block(0, 0, False, False)
        src = "\n".join(lines)
        return src + r.choice(["\n", "", "\n\n"])


# ---- mutação ao nível dos tokens ----
POOL = ["(", ")", "[", "]", ":", ",", ";", "=", "+", "-", "**", "not", "and", "or", "if", "else",
        "elif", "for", "in", "while", "def", "return", "pass", "break", "class", "lambda", "is",
        "x", "1", "1.5", "True", "None", "import", "from", "@", ".", "->", "==", "<", "+=", "{", "}",
        ":=", "...", "'s'", "\n", "\n    ", "\\\n"]


def token_spans(src):
    lines = src.splitlines(keepends=True)
    offs = [0]
    for ln in lines:
        offs.append(offs[-1] + len(ln))
    spans = []
    try:
        for tok in tokenize.generate_tokens(io.StringIO(src).readline):
            if tok.type in (tokenize.ENDMARKER, tokenize.DEDENT, tokenize.INDENT, tokenize.NL, tokenize.NEWLINE):
                continue
            (r1, c1), (r2, c2) = tok.start, tok.end
            spans.append((offs[r1 - 1] + c1, offs[r2 - 1] + c2))
    except (tokenize.TokenError, SyntaxError, IndexError):
        pass
    return spans


def token_fuzz(r, src):
    for _ in range(r.randint(1, 3)):
        spans = token_spans(src)
        if not spans:
            break
        a, b = r.choice(spans)
        c = r.random()
        if c < 0.3:
            src = src[:a] + src[b:]
        elif c < 0.55:
            src = src[:a] + " " + r.choice(POOL) + " " + src[a:]
        elif c < 0.75:
            src = src[:a] + r.choice(POOL) + src[b:]
        elif c < 0.9:
            src = src[:a] + src[a:b] + " " + src[a:]
        else:
            i = r.randrange(len(spans))
            if i + 1 < len(spans):
                (a1, b1), (a2, b2) = spans[i], spans[i + 1]
                src = src[:a1] + src[a2:b2] + src[b1:a2] + src[a1:b1] + src[b2:]
    return src


def deep_case(r):
    n = r.choice([1, 2, 999, 1000, 1001, 1002, r.randint(900, 1100), r.randint(1500, 2900)])
    k = r.choice(["neg", "not", "add", "call", "attr", "idx", "pow", "and", "cmp", "mix"])
    if k == "neg":
        e = "-" * n + "x"
    elif k == "not":
        e = "not " * n + "x"
    elif k == "add":
        e = "+".join(["a"] * n)
    elif k == "call":
        e = "a" + "()" * n
    elif k == "attr":
        e = "a" + ".b" * n
    elif k == "idx":
        e = "a" + "[0]" * n
    elif k == "pow":
        e = "**".join(["a"] * n)
    elif k == "and":
        e = " and ".join(["a"] * n)
    elif k == "cmp":
        e = "<".join(["a"] * n)
    else:
        e = "-~+" * (n // 3) + "f(" + "a+" * (n // 2) + "b)"
    return f"x = {e}\n"


# ---- execução e classificação ----
def run_ours(dump, path):
    try:
        p = subprocess.run([dump, path], capture_output=True, timeout=10)
    except subprocess.TimeoutExpired:
        return None, ["TIMEOUT"], -999, "timeout"
    out = p.stdout.decode("utf-8", "replace").split("\n")
    errs = [ln.split("\t")[1] if "\t" in ln else ln for ln in out if ln.startswith("ERROR")]
    sexpr = [ln.rstrip() for ln in out if ln and not ln.startswith("ERROR")]
    return sexpr, errs, p.returncode, p.stderr.decode("utf-8", "replace")


def py_parse(src):
    try:
        return ast.parse(src), None
    except (SyntaxError, ValueError, RecursionError, MemoryError) as e:
        return None, f"{type(e).__name__}: {getattr(e, 'msg', e)}"


def classify(src, path, dump, mode):
    with open(path, "w", newline="", encoding="utf-8") as f:
        f.write(src)
    py_src = src.replace("\r\n", "\n").replace("\r", "\n")
    tree, py_err = py_parse(py_src)
    exp, unsup = None, None
    if tree is not None:
        try:
            exp = module_sexpr(tree)
        except Unsupported as e:
            unsup = str(e)
        except RecursionError:
            unsup = "recursão no conversor"
    got, errs, rc, stderr = run_ours(dump, path)
    if rc not in (0, 1) or "panicked" in stderr or rc == -999:
        return "CRASH", stderr[:300] or f"rc={rc}"
    if mode == "deep" and tree is not None:
        md = max_depth_module(tree)
        accepted = not errs
        if accepted != (md <= MAX_DEPTH):
            return "LIMITE_ERRADO", f"profundidade={md} aceitámos={accepted} erros={errs[:2]}"
        if errs and not all(any(k in e for k in INTENTIONAL) for e in errs):
            return "MENSAGEM_NAO_INTENCIONAL", "; ".join(errs[:3])
        if not errs and exp is not None and got != exp:
            return "DIFERENCA_AST", "(profundo)"
        return ("ok" if not errs else "limite_intencional"), ""
    if py_err:
        return ("ambos_rejeitam", "") if errs else ("PY_REJEITA_NOS_ACEITAMOS", py_err)
    if errs:
        if all(any(k in e for k in INTENTIONAL) for e in errs):
            if mode == "valid" and unsup is None:
                return "REJEICAO_EM_VALIDO", "; ".join(errs[:3])
            return "divergencia_intencional", ""
        if unsup:
            return "MENSAGEM_NAO_INTENCIONAL", f"fora do subconjunto ({unsup}); nós: " + "; ".join(errs[:3])
        return "NOS_REJEITAMOS_PY_ACEITA", "; ".join(errs[:3])
    if unsup:
        return "ACEITAMOS_FORA_DO_SUBCONJUNTO", unsup
    if got != exp:
        j = next((i for i, (a, b) in enumerate(zip(got, exp)) if a != b), min(len(got), len(exp)))
        g = got[j] if j < len(got) else "<fim>"
        x = exp[j] if j < len(exp) else "<fim>"
        return "DIFERENCA_AST", f"item {j}\n    nós   : {g[:400]}\n    python: {x[:400]}"
    return "ok", ""


FAILS = {"CRASH", "PY_REJEITA_NOS_ACEITAMOS", "NOS_REJEITAMOS_PY_ACEITA", "ACEITAMOS_FORA_DO_SUBCONJUNTO",
         "DIFERENCA_AST", "MENSAGEM_NAO_INTENCIONAL", "LIMITE_ERRADO", "REJEICAO_EM_VALIDO"}


def main():
    dump, n, seed, mode = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
    work = f"/tmp/po_{mode}_{seed}"
    os.makedirs(work, exist_ok=True)
    r = random.Random(seed)
    g = G(r)
    cats, shown = Counter(), Counter()
    for i in range(n):
        if mode == "valid":
            src = g.program()
        elif mode == "wild":
            src = V.mutate_program(r, L.rnd_program(r))
        elif mode == "fuzz":
            src = token_fuzz(r, g.program())
        elif mode == "deep":
            src = deep_case(r)
        else:
            raise SystemExit(f"modo desconhecido {mode}")
        path = os.path.join(work, f"c{i}.cdr")
        cat, detail = classify(src, path, dump, mode)
        cats[cat] += 1
        if cat in FAILS and shown[cat] < 4:
            shown[cat] += 1
            print(f"### {cat} {path}\n  src={src[:300]!r}\n  {detail}")
    nfail = sum(v for k, v in cats.items() if k in FAILS)
    print(f"RESUMO {mode} seed={seed}: {nfail} falhas {dict(cats)}")
    STATUS.append(1 if nfail else 0)


if __name__ == "__main__":
    sys.setrecursionlimit(1_000_000)
    threading.stack_size(512 * 1024 * 1024)
    def guarded():
        try:
            main()
        except BaseException as e:
            print(f"RESUMO ERRO_DO_ORACULO: {type(e).__name__}: {str(e)[:300]}")
            STATUS.append(2)

    th = threading.Thread(target=guarded)
    th.start()
    th.join()
    sys.exit(STATUS[0] if STATUS else 2)
