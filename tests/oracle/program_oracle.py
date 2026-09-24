"""Oráculo diferencial de programas completos da Fase 1 contra o CPython 3.12.

Gera programas bem tipados do subconjunto (funções, recursão, ciclos, prints,
aritmética int/float/bool, comparações mistas, arg_int/arg_float) e:

  check : `cudrac check` tem de aceitar todos os bem tipados e rejeitar (exit 1)
          todas as mutações que violam as regras da §1.3/§6; se o CPython der
          NameError/UnboundLocalError/TypeError numa mutação, temos de a rejeitar.
  run   : `cudrac build` (debug e --release), corre com os mesmos argumentos e
          compara stdout + exit code com `python3` (PYTHONPATH com o shim).
  gen   : só o gerador contra o CPython (auto-teste: os programas têm de correr
          sem NameError/TypeError/RecursionError).

Uso: python3 program_oracle.py <cudrac|-> <N> <seed> <check|run|gen>

Os inteiros guardados (variáveis, parâmetros, retornos) ficam sempre com |v| < 2^20
(atribuições envolvidas em `% M`), e cada expressão tem um limite conhecido < 2^63:
nunca há overflow de i64, portanto debug e release têm de coincidir com o Python.
"""
import os
import random
import subprocess
import sys
from collections import Counter

I64 = 2**63 - 1
STORE = 2**20
VAR_BOUND = 2**21
MODS = [7, 10, 97, 1000, 65536, 1000003]
ARGS = ["7", "-3", "2.5", "1e3", " 42 ", "0", "-0.0"]
ARG_INT_OK = [1, 2, 5, 6]
INT_LITS = [0, 1, 2, 3, 5, 7, 10, 42, 100, 255, 1000, 65535, 99991]
BIG_INT_LITS = ["9223372036854775807", "-9223372036854775808", "9007199254740993", "9007199254740992",
                "4611686018427387904", "-4611686018427387905", "1285", "0x7fffffffffffffff", "0b1011", "0o777", "1_000_000"]
FLOAT_LITS = ["0.0", "1.0", "0.1", "0.5", "2.5", "3.14159", "1e10", "1e-05", "1e300", "123456789.125", "0.3",
              "9007199254740992.0", "9.223372036854776e18", "1.5e-320", "7.0", "100.0", "1e16", "2.0"]
SAFE_FDIV = ["2.5", "-0.1", "3.0", "0.001", "7.0", "-1.5", "1e-3", "0.3"]
FEXP = ["0.5", "2.0", "-1.0", "1.5", "-0.5", "3.0", "0.25"]


class Fn:
    def __init__(self, name, params, ret, prints):
        self.name = name
        self.params = params  # [(nome, tipo)]
        self.ret = ret        # "int" | "float" | "bool" | "None"
        self.annotate = True
        self.calls = set()
        self.prints = prints


class Ctx:
    """Âmbito de geração: uma função ou o main."""

    def __init__(self, fn, prefix):
        self.fn = fn
        self.prefix = prefix
        self.vars = {"int": [], "float": [], "bool": []}
        self.decls = []          # linhas de declaração no início do corpo
        self.readonly = set()    # d, contadores de while
        self.n = Counter()

    def new(self, ty, init, readonly=False, hidden=False):
        tag = {"int": "a", "float": "x", "bool": "b"}[ty] if not hidden else "g"
        name = f"{self.prefix}{tag}{self.n[tag]}"
        self.n[tag] += 1
        self.decls.append(Line(f"{name} = {init}", ("decl", name, ty)))
        if readonly:
            self.readonly.add(name)
        elif not hidden:
            self.vars[ty].append(name)
        return name

    def writable(self, ty):
        return [v for v in self.vars[ty] if v not in self.readonly]


class Line:
    def __init__(self, text, tag=None, indent=0):
        self.text = text
        self.tag = tag
        self.indent = indent


def ind(lines, k):
    return [Line(ln.text, ln.tag, ln.indent + k) for ln in lines]


class Gen:
    def __init__(self, r):
        self.r = r
        self.fns = []
        self.uses_args = False

    # ---------- expressões ----------
    def wrap_mod(self, src, bound, limit):
        if bound <= limit:
            return src, bound
        ms = [m for m in MODS if m - 1 <= limit]
        m = self.r.choice(ms) if ms else 1
        return f"({src} % {m})", m - 1

    def divisor(self, ctx, d):
        r = self.r
        if r.random() < 0.85:
            k = r.choice([2, 3, 5, 7, 10, 1000])
            e, _ = self.gi(ctx, d - 1, STORE)
            return f"({e} % {k} + 1)", k
        if r.random() < 0.5:
            return r.choice(["-1", "1", "-7", "3"]), 7
        return self.gi(ctx, d - 1, 2**31)

    def int_leaf(self, ctx, limit):
        r = self.r
        c = r.random()
        if limit >= I64 and c < 0.12:
            return r.choice(BIG_INT_LITS), I64
        if c < 0.4:
            v = r.choice([x for x in INT_LITS if x <= limit] or [0])
            return str(v), v
        pool = ctx.vars["int"] + [v for v in ctx.readonly]
        if c < 0.85 and pool:
            v = r.choice(pool)
            return self.wrap_mod(v, VAR_BOUND, limit)
        if c < 0.92:
            self.uses_args = True
            if r.random() < 0.03:
                return r.choice(["arg_int(3)", "arg_int(9)"]), STORE
            return f"arg_int({r.choice(ARG_INT_OK)})", 42
        return str(r.choice([x for x in INT_LITS if x <= limit] or [0])), limit

    def call(self, ctx, d, want):
        r = self.r
        cands = [f for f in self.fns if f.ret == want]
        if not cands:
            return None
        f = r.choice(cands)
        if ctx.fn is not None:
            ctx.fn.calls.add(f.name)
            depth = "d - 1"
        else:
            depth = str(r.randint(0, 3))
        args = [depth]
        for _, ty in f.params:
            if ty == "int":
                args.append(self.gi(ctx, d - 1, STORE)[0])
            elif ty == "float":
                args.append(self.gf(ctx, d - 1))
            else:
                args.append(self.gb(ctx, d - 1))
        return f"{f.name}({', '.join(args)})"

    def gi(self, ctx, d, limit):
        """Expressão int com |valor| <= limite garantido. Devolve (texto, limite)."""
        src, bound = self.gi_raw(ctx, d, limit)
        return self.wrap_mod(src, bound, limit)

    def gi_raw(self, ctx, d, limit):
        r = self.r
        if d <= 0 or r.random() < 0.25:
            return self.int_leaf(ctx, limit)
        c = r.random()
        sub = 2**31
        if c < 0.3:
            op = r.choice(["+", "-", "*"])
            a, ba = self.gi(ctx, d - 1, sub)
            b, bb = self.gi(ctx, d - 1, sub)
            bound = ba + bb if op != "*" else ba * bb
            return self.wrap_mod(f"({a} {op} {b})", bound, limit)
        if c < 0.42:
            op = r.choice(["//", "%"])
            a, ba = self.gi(ctx, d - 1, I64 if r.random() < 0.3 else sub)
            q, bq = self.divisor(ctx, d)
            if op == "//" and ba > 2**62 and not q.endswith(" + 1)"):
                q, bq = "3", 3
            bound = ba if op == "//" else bq
            return self.wrap_mod(f"({a} {op} {q})", bound, limit)
        if c < 0.48:
            a, ba = self.gi(ctx, d - 1, 15)
            if r.random() < 0.5:
                e = r.randint(0, 6)
                es, emax = str(e), e
            else:
                x, _ = self.gi(ctx, d - 1, STORE)
                es, emax = f"({x} % 4)", 3
            return self.wrap_mod(f"({a} ** {es})", max(ba, 1) ** emax, limit)
        if c < 0.56:
            a, ba = self.gi(ctx, d - 1, STORE)
            if r.random() < 0.5:
                k = r.randint(0, 24)
                return self.wrap_mod(f"({a} << {k})", ba * 2**k, limit)
            if r.random() < 0.05:
                return f"({a} >> -1)", ba
            if r.random() < 0.5:
                x, _ = self.gi(ctx, d - 1, STORE)
                return f"({a} >> ({x} % 70))", ba
            return f"({a} >> {r.randint(0, 80)})", ba
        if c < 0.64:
            op = r.choice(["&", "|", "^"])
            a, ba = self.gi(ctx, d - 1, sub)
            b, bb = self.gi(ctx, d - 1, sub)
            return self.wrap_mod(f"({a} {op} {b})", 2 ** max(ba, bb).bit_length(), limit)
        if c < 0.72:
            op = r.choice(["-", "+", "~"])
            a, ba = self.gi(ctx, d - 1, sub)
            return self.wrap_mod(f"({op}{a})", ba + 1, limit)
        if c < 0.76:
            a, ba = self.gi(ctx, d - 1, sub)
            return self.wrap_mod(f"abs({a})", ba, limit)
        if c < 0.82:
            f = self.gf(ctx, d - 1)
            return self.wrap_mod(f"int({f} % 1000.0)", 1000, limit)
        if c < 0.85:
            return f"int({self.gb(ctx, d - 1)})", 1
        if c < 0.88:
            return f"int({self.gi(ctx, d - 1, limit)[0]})", limit
        call = self.call(ctx, d, "int")
        if call:
            return self.wrap_mod(call, STORE, limit)
        return self.int_leaf(ctx, limit)

    def gf(self, ctx, d):
        r = self.r
        if d <= 0 or r.random() < 0.25:
            c = r.random()
            if c < 0.4 or not ctx.vars["float"]:
                s = r.choice(FLOAT_LITS)
                return f"(-{s})" if r.random() < 0.2 else s
            if c < 0.9:
                return r.choice(ctx.vars["float"])
            self.uses_args = True
            return f"arg_float({r.randint(1, len(ARGS))})"
        c = r.random()
        if c < 0.35:
            op = r.choice(["+", "-", "*", "/", "//", "%"])
            a_float = r.random() < 0.75
            a = self.gf(ctx, d - 1) if a_float else self.gi(ctx, d - 1, I64)[0]
            if op in ("/", "//", "%") and r.random() < 0.8:
                b = r.choice(SAFE_FDIV)
            elif not a_float or r.random() < 0.7:
                b = self.gf(ctx, d - 1)
            else:
                b = self.gi(ctx, d - 1, I64)[0]
            return f"({a} {op} {b})"
        if c < 0.45:
            a, _ = self.gi(ctx, d - 1, I64)
            q, _ = self.divisor(ctx, d) if r.random() < 0.85 else self.gi(ctx, d - 1, I64)
            return f"({a} / {q})"
        if c < 0.55:
            k = r.random()
            if k < 0.4:
                return f"(abs({self.gf(ctx, d - 1)}) ** {r.choice(FEXP)})"
            if k < 0.7:
                return f"({self.gf(ctx, d - 1)} ** {r.choice(['2', '3', '2.0', '-1', '0'])})"
            return f"(abs({self.gi(ctx, d - 1, STORE)[0]}) ** {r.choice(FEXP)})"
        if c < 0.63:
            return f"(-{self.gf(ctx, d - 1)})" if r.random() < 0.7 else f"abs({self.gf(ctx, d - 1)})"
        if c < 0.73:
            return f"float({self.gi(ctx, d - 1, I64)[0]})"
        if c < 0.76:
            return f"float({self.gb(ctx, d - 1)})"
        call = self.call(ctx, d, "float")
        return call or self.gf(ctx, 0)

    def num(self, ctx, d):
        if self.r.random() < 0.5:
            return self.gi(ctx, d, I64)[0]
        return self.gf(ctx, d)

    def gb(self, ctx, d):
        r = self.r
        if d <= 0 or r.random() < 0.2:
            if ctx.vars["bool"] and r.random() < 0.6:
                return r.choice(ctx.vars["bool"])
            return r.choice(["True", "False"])
        c = r.random()
        if c < 0.4:
            parts = [self.num(ctx, d - 1) for _ in range(r.randint(2, 4))]
            out = parts[0]
            for p in parts[1:]:
                out += f" {r.choice(['<', '<=', '>', '>=', '==', '!='])} {p}"
            return f"({out})"
        if c < 0.47:
            return f"({self.gb(ctx, d - 1)} {r.choice(['==', '!='])} {self.gb(ctx, d - 1)})"
        if c < 0.57:
            x = r.random()
            e = self.gb(ctx, d - 1) if x < 0.5 else self.num(ctx, d - 1)
            return f"(not {e})"
        if c < 0.75:
            op = r.choice(["and", "or"])
            return f"({self.gb(ctx, d - 1)} {op} {self.gb(ctx, d - 1)})"
        if c < 0.85:
            return f"bool({self.num(ctx, d - 1)})"
        call = self.call(ctx, d, "bool")
        return call or r.choice(["True", "False"])

    def cond(self, ctx, d):
        r = self.r
        if r.random() < 0.8:
            return self.gb(ctx, d)
        return self.num(ctx, d)

    def gexpr(self, ctx, ty, d):
        if ty == "int":
            return self.gi(ctx, d, STORE)[0]
        if ty == "float":
            return self.gf(ctx, d)
        return self.gb(ctx, d)

    # ---------- instruções ----------
    def block(self, ctx, depth, loop, budget):
        r = self.r
        out = []
        for _ in range(r.randint(1, 4)):
            if budget[0] <= 0:
                break
            budget[0] -= 1
            out += self.stmt(ctx, depth, loop, budget)
        return out or [Line("pass")]

    def stmt(self, ctx, depth, loop, budget):
        r = self.r
        c = r.random()
        E = 3
        if c < 0.18:
            ty = r.choice(["int", "float", "bool"])
            vs = ctx.writable(ty)
            if vs:
                v = r.choice(vs)
                return [Line(f"{v} = {self.gexpr(ctx, ty, E)}", ("assign", v, ty))]
        if c < 0.28:
            vs = ctx.writable("int")
            if vs:
                v = r.choice(vs)
                op = r.choice(["+=", "-=", "*=", "<<=", "**=", "//=", "%=", ">>=", "&=", "|=", "^="])
                if op == "<<=":
                    rhs = str(r.randint(0, 20))
                elif op == "**=":
                    rhs = str(r.randint(0, 2))
                elif op in ("//=", "%="):
                    rhs = self.divisor(ctx, E)[0]
                elif op == ">>=":
                    rhs = str(r.randint(0, 70))
                else:
                    rhs = self.gi(ctx, E, STORE)[0]
                grow = op in ("+=", "-=", "*=", "<<=", "**=")
                s = f"{v} {op} {rhs}"
                if grow:
                    s += f"; {v} %= {r.choice(MODS)}" if r.random() < 0.5 else ""
                    lines = [Line(s, ("aug", v, "int"))]
                    if not s.endswith(tuple(f"%= {m}" for m in MODS)):
                        lines.append(Line(f"{v} %= {r.choice(MODS)}"))
                    return lines
                return [Line(s, ("aug", v, "int"))]
        if c < 0.34:
            vs = ctx.writable("float")
            if vs:
                v = r.choice(vs)
                op = r.choice(["+=", "-=", "*=", "/=", "//=", "%=", "**="])
                if op == "**=":
                    rhs = r.choice(["2", "2.0", "3"])
                elif op in ("/=", "//=", "%=") and r.random() < 0.8:
                    rhs = r.choice(SAFE_FDIV)
                else:
                    rhs = self.gf(ctx, E) if r.random() < 0.7 else self.gi(ctx, E, STORE)[0]
                return [Line(f"{v} {op} {rhs}", ("aug", v, "float"))]
        if c < 0.5:
            args = []
            for _ in range(r.randint(0, 4)):
                k = r.random()
                if k < 0.3:
                    args.append(self.gi(ctx, E, I64)[0])
                elif k < 0.6:
                    args.append(self.gf(ctx, E))
                elif k < 0.8:
                    args.append(self.gb(ctx, E))
                else:
                    call = self.call(ctx, E, "None") or self.call(ctx, E, r.choice(["int", "float", "bool"]))
                    args.append(call or "0")
            return [Line(f"print({', '.join(args)})", ("print",))]
        if c < 0.55:
            call = self.call(ctx, E, r.choice(["None", "int", "float", "bool"]))
            if call:
                return [Line(call, ("callstmt",))]
        if depth < 3 and c < 0.68:
            out = [Line(f"if {self.cond(ctx, E)}:")] + ind(self.block(ctx, depth + 1, loop, budget), 4)
            for _ in range(r.choice([0, 0, 1, 2])):
                out += [Line(f"elif {self.cond(ctx, E)}:")] + ind(self.block(ctx, depth + 1, loop, budget), 4)
            if r.random() < 0.5:
                out += [Line("else:")] + ind(self.block(ctx, depth + 1, loop, budget), 4)
            return out
        if depth < 3 and c < 0.75:
            k = ctx.new("int", "0", readonly=True)
            lim = r.randint(0, 5 if depth == 0 else 3)
            head = f"while {k} < {lim}:" if r.random() < 0.6 else f"while {k} < {lim} and {self.gb(ctx, E)}:"
            body = [Line(f"{k} += 1")] + self.block(ctx, depth + 1, True, budget)
            return [Line(f"{k} = 0"), Line(head)] + ind(body, 4)
        if depth < 3 and c < 0.84:
            vs = ctx.writable("int")
            v = r.choice(vs) if vs and r.random() < 0.7 else ctx.new("int", "0")
            a = self.gi(ctx, 2, STORE)[0]
            b = self.gi(ctx, 2, STORE)[0]
            k = r.random()
            if k < 0.35:
                rng = f"range({a} % 8)"
            elif k < 0.6:
                rng = f"range({a} % 6, {b} % 9)"
            else:
                step = r.choice(["1", "2", "3", "-1", "-2", f"({a} % 5 - 2)"])
                rng = f"range({a} % 7 - 3, {b} % 7 - 3, {step})"
            body = self.block(ctx, depth + 1, True, budget)
            if r.random() < 0.2:
                body.insert(0, Line(f"{v} = {v} + 100"))
            return [Line(f"for {v} in {rng}:")] + ind(body, 4)
        if loop and c < 0.88:
            return [Line(f"if {self.gb(ctx, 2)}:"), Line(r.choice(["break", "continue"]), None, 4)]
        if ctx.fn is not None and c < 0.9:
            return [Line(f"if {self.gb(ctx, 2)}:")] + ind(self.ret_line(ctx), 4)
        if c < 0.93:
            t = f"{ctx.prefix}t{ctx.n['t']}"
            ctx.n["t"] += 1
            ty = r.choice(["int", "float", "bool"])
            return [Line(f"if {self.gb(ctx, 2)}:"), Line(f"{t} = {self.gexpr(ctx, ty, 2)}", None, 4),
                    Line("else:"), Line(f"{t} = {self.gexpr(ctx, ty, 2)}", None, 4),
                    Line(f"print({t})")]
        if c < 0.95:
            g = ctx.new("int", "0", hidden=True)
            lo =r.choice(["9223372036854775800", "-9223372036854775807", "9223372036854775000"])
            hi = r.choice(["9223372036854775807", "-9223372036854775800", "-9223372036854775808"])
            step = r.choice(["3", "1", "-2", "1000", "-1"])
            return [Line(f"for {g} in range({lo}, {hi}, {step}):"), Line(f"print({g})", None, 4), Line(f"print({g})")]
        return [Line(f"print({self.gi(ctx, E, I64)[0]}, {self.gf(ctx, E)})", ("print",))]

    def ret_line(self, ctx):
        f = ctx.fn
        if f.ret == "None":
            return [Line("return", ("ret", "None"))]
        return [Line(f"return {self.gexpr(ctx, f.ret, 3)}", ("ret", f.ret))]

    # ---------- programa ----------
    def program(self):
        r = self.r
        self.fns = []
        self.uses_args = False
        for i in range(r.randint(0, 4)):
            params = [(f"p{j}", r.choice(["int", "float", "bool"])) for j in range(r.randint(0, 3))]
            self.fns.append(Fn(f"f{i}", params, r.choice(["int", "float", "bool", "None"]), r.random() < 0.5))
        bodies = []
        for f in self.fns:
            ctx = Ctx(f, "")
            ctx.readonly.add("d")
            for p, ty in f.params:
                ctx.vars[ty].append(p)
            for ty, init in (("int", "5"), ("float", "1.5"), ("bool", "True")):
                for _ in range(r.randint(0, 2)):
                    ctx.new(ty, init if r.random() < 0.5 else self.gexpr(ctx, ty, 1))
            body = self.block(ctx, 1, False, [r.randint(3, 10)])
            base = {"int": str(r.choice(INT_LITS)), "float": r.choice(FLOAT_LITS), "bool": r.choice(["True", "False"])}
            guard = [Line("if d <= 0:"), Line("return" if f.ret == "None" else f"return {base[f.ret]}", ("ret", f.ret), 4)]
            tail = [] if f.ret == "None" and r.random() < 0.6 else self.ret_line(ctx)
            bodies.append((f, guard + ctx.decls + body + tail))
        # anotações de retorno: obrigatórias em funções envolvidas em ciclos de chamadas
        names = {f.name: f for f in self.fns}
        for f in self.fns:
            seen, stack = set(), list(f.calls)
            while stack:
                g = stack.pop()
                if g in seen:
                    continue
                seen.add(g)
                stack += list(names[g].calls)
            cyclic = f.name in seen
            f.annotate = cyclic or r.random() < 0.7
        mctx = Ctx(None, "m_")
        mctx.new("int", "0")
        for ty, init in (("int", "3"), ("float", "0.5"), ("bool", "False")):
            for _ in range(r.randint(0, 2)):
                mctx.new(ty, init if r.random() < 0.5 else self.gexpr(mctx, ty, 1))
        main = self.block(mctx, 0, False, [r.randint(4, 14)])
        lines = []
        if self.uses_args or r.random() < 0.3:
            lines.append(Line("from cudra import arg_int, arg_float", ("import",)))
        for f, body in bodies:
            ps = ", ".join(["d: int"] + [f"{p}: {ty}" for p, ty in f.params])
            ret = f" -> {f.ret}" if f.annotate else ""
            lines.append(Line(f"def {f.name}({ps}){ret}:", ("def", f.name)))
            lines += ind(body, 4)
        lines.append(Line("", ("main",)))
        lines += mctx.decls + main
        return lines


def render(lines):
    return "\n".join(" " * ln.indent + ln.text for ln in lines) + "\n"


# ---------- mutações (modo check) ----------
def mutate(r, g, lines):
    """Devolve (nome, linhas mutadas, exige_rejeição)."""
    kinds = ["int_para_float", "float_para_int", "bool_aritmetica", "sem_declaracao", "aridade", "funcao_desconhecida",
             "global_em_funcao", "return_errado", "and_int", "chamada_antes_do_def", "arg_float_em_int", "none_aritmetica",
             "bool_em_int", "string", "var_do_for_talvez_indefinida", "var_so_num_ramo", "sombra_builtin",
             "funcao_duplicada", "funcao_aninhada", "return_no_topo", "break_fora", "decorador", "alvo_indice",
             "param_sem_anotacao", "import_em_funcao", "import_invalido", "nome_importado_invalido",
             "int_literal_grande", "uso_antes_no_main", "recursao_sem_retorno_anotado"]
    k = r.choice(kinds)
    L = list(lines)
    main_at = next(i for i, ln in enumerate(L) if ln.tag == ("main",))

    def insert_main(text):
        L.append(Line(text))

    def fn_lines(name):
        s = next(i for i, ln in enumerate(L) if ln.tag == ("def", name))
        return s

    if k == "int_para_float":
        c = [i for i, ln in enumerate(L) if ln.tag and ln.tag[0] == "assign" and ln.tag[2] == "int"]
        if c:
            i = r.choice(c)
            L[i] = Line(f"{L[i].tag[1]} = 1.5", None, L[i].indent)
            return k, L, True
    if k == "float_para_int":
        c = [i for i, ln in enumerate(L) if ln.tag and ln.tag[0] in ("assign",) and ln.tag[2] == "float"]
        if c:
            i = r.choice(c)
            L[i] = Line(f"{L[i].tag[1]} = 3", None, L[i].indent)
            return k, L, True
    if k == "bool_aritmetica":
        insert_main("m_a0 = True + 1")
        return k, L, True
    if k == "sem_declaracao":
        c = [i for i, ln in enumerate(L) if ln.tag and ln.tag[0] == "decl"]
        if c:
            del L[r.choice(c)]
            return k, L, None
    if k == "aridade" and g.fns:
        f = r.choice(g.fns)
        insert_main(f"{f.name}({', '.join(['0'] * (len(f.params) + 2))})")
        return k, L, True
    if k == "funcao_desconhecida":
        insert_main("zz_nao_existe(1)")
        return k, L, True
    if k == "global_em_funcao" and g.fns:
        s = fn_lines(r.choice(g.fns).name)
        L.insert(s + 1, Line("print(m_a0)", None, 4))
        return k, L, True
    if k == "return_errado" and g.fns:
        f = r.choice(g.fns)
        bad = {"int": "1.5", "float": "1", "bool": "1", "None": "1"}[f.ret]
        s = fn_lines(f.name)
        L.insert(s + 1, Line(f"if d == 12345:", None, 4))
        L.insert(s + 2, Line(f"return {bad}", None, 8))
        return k, L, True
    if k == "and_int":
        insert_main("print(1 and 2)")
        return k, L, True
    if k == "chamada_antes_do_def" and g.fns:
        f = g.fns[-1]
        at = 1 if L and L[0].tag == ("import",) else 0
        args = ["0"] + [{"int": "1", "float": "1.0", "bool": "True"}[t] for _, t in f.params]
        L.insert(at, Line(f"{f.name}({', '.join(args)})"))
        return k, L, True
    if k == "arg_float_em_int":
        c = [f for f in g.fns if any(t == "int" for _, t in f.params)]
        if c:
            f = r.choice(c)
            args = ["0"] + [{"int": "1.5", "float": "1.0", "bool": "True"}[t] for _, t in f.params]
            insert_main(f"{f.name}({', '.join(args)})")
            return k, L, True
    if k == "none_aritmetica":
        c = [f for f in g.fns if f.ret == "None"]
        if c:
            f = r.choice(c)
            args = ["0"] + [{"int": "1", "float": "1.0", "bool": "True"}[t] for _, t in f.params]
            insert_main(f"print({f.name}({', '.join(args)}) + 1)")
            return k, L, True
    if k == "bool_em_int":
        insert_main("m_a0 = False")
        return k, L, True
    if k == "string":
        insert_main('print("ola")')
        return k, L, True
    if k == "var_do_for_talvez_indefinida":
        insert_main("for m_q in range(m_a0):\n    pass\nprint(m_q)")
        return k, L, True
    if k == "var_so_num_ramo":
        insert_main("if m_a0 > 100:\n    m_z = 1\nprint(m_z)")
        return k, L, True
    if k == "sombra_builtin":
        at = 1 if L and L[0].tag == ("import",) else 0
        L.insert(at, Line("def abs(d: int) -> int:\n    return d"))
        return k, L, True
    if k == "funcao_duplicada" and g.fns:
        at = 1 if L and L[0].tag == ("import",) else 0
        L.insert(at, Line(f"def {g.fns[0].name}(d: int) -> int:\n    return d"))
        return k, L, True
    if k == "funcao_aninhada" and g.fns:
        s = fn_lines(r.choice(g.fns).name)
        L.insert(s + 1, Line("def interna(q: int) -> int:\n        return q", None, 4))
        return k, L, True
    if k == "return_no_topo":
        insert_main("return")
        return k, L, True
    if k == "break_fora":
        insert_main("break")
        return k, L, True
    if k == "decorador" and g.fns:
        s = fn_lines(r.choice(g.fns).name)
        L.insert(s, Line("@deco"))
        return k, L, True
    if k == "alvo_indice":
        insert_main("m_a0[0] = 1")
        return k, L, True
    if k == "param_sem_anotacao":
        at = 1 if L and L[0].tag == ("import",) else 0
        L.insert(at, Line("def sem_anot(q) -> int:\n    return 1"))
        return k, L, True
    if k == "import_em_funcao" and g.fns:
        s = fn_lines(r.choice(g.fns).name)
        L.insert(s + 1, Line("from cudra import arg_int", None, 4))
        return k, L, True
    if k == "import_invalido":
        L.insert(0, Line("from math import sqrt"))
        return k, L, True
    if k == "nome_importado_invalido":
        L.insert(0, Line("from cudra import arg_str"))
        return k, L, True
    if k == "int_literal_grande":
        insert_main("print(9223372036854775808)")
        return k, L, True
    if k == "uso_antes_no_main":
        insert_main("print(m_novo)\nm_novo = 1")
        return k, L, True
    if k == "recursao_sem_retorno_anotado":
        at = 1 if L and L[0].tag == ("import",) else 0
        L.insert(at, Line("def rr(d: int):\n    if d <= 0:\n        return rr(d - 1)\n    return rr(d - 1)"))
        return k, L, True
    return None


# ---------- execução ----------
SHIM = "/tmp/po_shim"


def ensure_shim():
    os.makedirs(f"{SHIM}/cudra", exist_ok=True)
    with open(f"{SHIM}/cudra/__init__.py", "w") as f:
        f.write("import sys\n\n\ndef arg_int(i):\n    return int(sys.argv[i])\n\n\ndef arg_float(i):\n    return float(sys.argv[i])\n")


def run(cmd, timeout=10, env=None):
    try:
        p = subprocess.run(cmd, capture_output=True, timeout=timeout, env=env)
    except subprocess.TimeoutExpired:
        return None, b"", b"TIMEOUT"
    return p.returncode, p.stdout, p.stderr


def run_python(path):
    env = dict(os.environ, PYTHONPATH=SHIM, PYTHONHASHSEED="0")
    return run(["python3", path] + ARGS, timeout=10, env=env)


PY_BAD = (b"NameError", b"UnboundLocalError", b"TypeError", b"RecursionError", b"SyntaxError", b"AttributeError")


def main():
    cudrac, n, seed, mode = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
    ensure_shim()
    work = f"/tmp/prog_{mode}_{seed}"
    os.makedirs(work, exist_ok=True)
    r = random.Random(seed)
    g = Gen(r)
    cats, shown = Counter(), Counter()
    FAILS = {"GERADOR_INVALIDO", "REJEITOU_VALIDO", "ACEITOU_INVALIDO", "CRASH", "COMPILACAO_FALHOU",
             "STDOUT_DIFERENTE", "EXIT_DIFERENTE", "TIMEOUT_NOSSO", "ACEITOU_MAS_PYTHON_FALHA_TIPOS"}

    def report(cat, path, detail):
        cats[cat] += 1
        if cat in FAILS and shown[cat] < 5:
            shown[cat] += 1
            print(f"### {cat} {path}\n{detail}")

    for i in range(n):
        lines = g.program()
        src = render(lines)
        path = f"{work}/p{i}.cdr"
        with open(path, "w") as f:
            f.write(src)
        prc, pout, perr = run_python(path)
        if prc is None or len(pout) > 2_000_000:
            report("python_lento", path, "")
            continue
        if any(b in perr for b in PY_BAD):
            report("GERADOR_INVALIDO", path, perr.decode(errors="replace")[-600:])
            continue
        if mode == "gen":
            report(f"py_exit_{prc}", path, "")
            continue
        if mode == "check":
            rc, _, err = run([cudrac, "check", path])
            if rc is None or rc not in (0, 1) or b"panicked" in err:
                report("CRASH", path, f"rc={rc} {err.decode(errors='replace')[-600:]}")
            elif rc != 0:
                report("REJEITOU_VALIDO", path, err.decode(errors="replace")[-1200:])
            else:
                report("aceite_ok", path, "")
            m = mutate(r, g, lines)
            if m is None:
                continue
            kind, ml, must_reject = m
            mpath = f"{work}/p{i}_{kind}.cdr"
            msrc = render(ml)
            with open(mpath, "w") as f:
                f.write(msrc)
            rc, _, err = run([cudrac, "check", mpath])
            if rc is None or rc not in (0, 1) or b"panicked" in err:
                report("CRASH", mpath, f"rc={rc} {err.decode(errors='replace')[-600:]}")
                continue
            if must_reject is None:
                mprc, _, mperr = run_python(mpath)
                if mprc is not None and any(b in mperr for b in (b"NameError", b"UnboundLocalError")) and rc == 0:
                    report("ACEITOU_MAS_PYTHON_FALHA_TIPOS", mpath, mperr.decode(errors="replace")[-400:])
                else:
                    report(f"mutacao_{kind}_ok", mpath, "")
            elif rc == 0:
                report("ACEITOU_INVALIDO", mpath, f"mutação {kind}")
            else:
                report(f"mutacao_{kind}_ok", mpath, "")
            continue
        # run
        for prof in ("debug", "release"):
            exe = f"{work}/p{i}_{prof}"
            cmd = [cudrac, "build", path, "-o", exe] + (["--release"] if prof == "release" else [])
            rc, _, err = run(cmd, timeout=60)
            if rc != 0:
                report("COMPILACAO_FALHOU", path, f"{prof} rc={rc} {err.decode(errors='replace')[-1200:]}")
                break
            orc, oout, oerr = run([exe] + ARGS, timeout=10)
            if orc is None:
                report("TIMEOUT_NOSSO", path, prof)
            elif orc not in (0, 1):
                report("CRASH", path, f"{prof} rc={orc} {oerr.decode(errors='replace')[-400:]}")
            elif oout != pout:
                a, b = pout.decode(errors="replace").split("\n"), oout.decode(errors="replace").split("\n")
                j = next((k for k, (x, y) in enumerate(zip(a, b)) if x != y), min(len(a), len(b)))
                report("STDOUT_DIFERENTE", path, f"{prof} linha {j + 1}: python={a[j] if j < len(a) else '<fim>'!r} "
                                                 f"nós={b[j] if j < len(b) else '<fim>'!r}")
            elif orc != prc:
                report("EXIT_DIFERENTE", path, f"{prof} python={prc} nós={orc} {oerr.decode(errors='replace')[-300:]} "
                                               f"| py: {perr.decode(errors='replace')[-300:]}")
            else:
                report(f"ok_{prof}", path, "")
    nfail = sum(v for k, v in cats.items() if k in FAILS)
    print(f"RESUMO programas {mode} seed={seed}: {nfail} falhas {dict(sorted(cats.items()))}")
    sys.exit(1 if nfail else 0)


if __name__ == "__main__":
    main()
