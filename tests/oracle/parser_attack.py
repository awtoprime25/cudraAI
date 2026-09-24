"""Ataques ao parser: casos escritos à mão contra o ast.parse do CPython, entradas
patológicas (tempo) e truncagem de cada caso em cada posição.

Uso: python3 parser_attack.py <dump_ast>   (exit 1 se houver falhas)
"""
import os
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import parser_oracle as P

SLOW = 2.0

CASES = [
    # subconjunto: o dump tem de ser igual
    "x = -1", "x = --1", "x = +-~1", "x = 2**-1", "x = a**-b**c", "x = -a ** -b", "x = (a < b) < c", "x = a < (b < c)",
    "x = a < b <= c != d", "x = not not a", "x = not a == b", "x = a and b or c and d", "x = a or (b or c)",
    "x = (a and b) and c", "x = a and (b or c) and d", "x = ~a.b", "x = 1 .real", "x = 1.0.real", "x = 1..real",
    "x = (a)(b)", "x = a.b.c(1)(2)[3]", "x = f(a,)", "x = f()", "x = f(\n a,\n b,\n)", "x = a[b][c]", "x = a[-1]",
    "if a: pass\nelif b: pass\nelse: pass", "if (a): x = 1; y = 2;", "while a: break; continue", "def f(): return",
    "def f() -> None:\n    return\n", "def f(a: Array[Array[f32]],) -> Array[int,]:\n    pass\n",
    "@a.b(1)(2)\n@c\ndef f(): pass", "@d\n\n# c\ndef f(a, b: int): pass", "@(a)\ndef f(): pass", "@a + b\ndef f(): pass",
    "x = 0x_ff + 0o17 + 0b1 + 1_000 + 00 + 0XAB + 0B_1", "x = 18446744073709551615", "x = 1e400", "x = 1_0.5e-3",
    "x = 1.e5", "x = .5e-5", "x = 4.9e-324", "x = 2.2250738585072014e-308", "x = 1e-400", "x = 9007199254740993.0",
    "x = 0e0", "x = 1E+3", "x = 1 \\\n + 2\n", "x = (1 +\n 2)\n", "if a:\n\n    # c\n    x = 1\n\n# c\ny = 2\n",
    "", "\n\n", "# só comentário", "x = 1", "x = 1\r\ny = 2\r\n", "if a:\r\n    x = 1\r\n", "(x) = 1", "((x)) += 1",
    "for (a) in b: pass", "for i in range(10):\n    if i: continue\n    else: break\n", "x: int = 1", "(x): int = 1",
    "x: Array[f32, i32] = 1", "x: None = y", "x[0] = 1", "f(a)[b] = 1", "x[i] += 1", "a.b[c] = 1", "return 1", "return",
    "break", "continue", "import cudra", "from cudra import arg_int, arg_float", "from a import (b,)", "from a import (b, c)",
    "def f(a, a): pass", "def f(a): import x", "match = 1", "match(x)", "match: int = 1", "type = 1", "type(x)",
    "type.x[0] = 1", "case = 1", "_ = 1", "print(match, case, type, _)", "x = a if\\\n", "x = 1;", "x = 1; y = 2",
    "if a: x = 1; y = 2\nelse: z = 3", "def f(a: int, b: float) -> float:\n    return a / b\n",
    "while x < 10:\n    x += 1\n    if x == 5:\n        break\n",
    "x = a <<= b" if False else "x <<= 1", "x >>= 1", "x **= 2", "x //= 2", "x %= 2", "x &= 1", "x |= 1", "x ^= 1",
    "x = a @b" if False else "x = a|b^c&d<<e>>f+g-h*i/j//k%l**m",
    # fora do subconjunto: todos os erros têm de ser intencionais
    "class A:\n    x = 1\n    def f(self): pass\ny = 2\n",
    "try:\n    x = 1\nexcept E:\n    pass\nelse:\n    y = 1\nfinally:\n    z = 1\nw = 2\n",
    "if a is b:\n    x = 1\nelif c:\n    y = 2\nelse:\n    z = 3\nw = 1\n",
    "while a:\n    pass\nelse:\n    pass\nx = 1\n", "for a, b in c:\n    pass\nelse:\n    pass\n",
    "for x in y:\n    pass\nelse:\n    pass\nz = 1\n", "if a:\n    def g():\n        pass\nelse:\n    x = 1\n",
    "def f():\n    def g():\n        pass\n    return 1\n", "@d\nclass A:\n    pass\n",
    "match x:\n    case 1:\n        pass\n    case _:\n        pass\nx = 1\n", "match (x):\n    case 1: pass",
    "match -x:\n case 1: pass", "type X = int", "type X[T] = list[T]", "with a as b, c as d:\n    pass\n",
    "async def f():\n    await x\n", "x = [i for i in range(3)]\n", "x = {'a': 1}\n", "print(f'{x}')\n",
    "print('a', 'b')\n", "x = 1 if a else 2\ny = 3\n", "x = lambda: (yield)", "del a, b", "global a, b",
    "nonlocal a", "assert a, 'm'", "raise E from e", "x = a[1:2, ::3]", "x = a[1:2]", "x = a[1, 2]",
    "def f(a, *, b=1, **k) -> int:\n    return 1\n", "def f(a, /, b):\n    pass", "def f(a=1): pass",
    "def f(*a): pass", "def f[T](a): pass", "def f(a: 'int'): pass", "def f(a: x.y): pass", "def f() -> x.y: pass",
    "import a.b.c as d", "import a, b", "import a as b", "from ..a import (b as c, d)", "from . import a",
    "from a import *", "x: int", "x: int\ny = 1", "a.b: int", "a.b: int = 1", "x[0]: int = 1", "x: 1 + 2 = 3",
    "x: Array[1] = 1", "x: Array[int][int] = 1", "x: Array[()] = 1", "x = *a, b", "print(*a, sep='')",
    "f(a)(b)[c].d = 1", "a.b = 1", "a.b += 1", "a = b = 1", "a, b = 1, 2", "x = 1, 2", "return a, b", "x = ()",
    "x = (a,)", "x = (y := 1)", "x = a @ b", "x @= 1", "x = ...", "x = a is not b", "x = a not in b",
    "x = a in b", "x = not a if b else c", "x = a < b if c else d", "x = 18446744073709551616", "x = 1j",
    "x = 'a' 'b'", "x = b'a'", "f(a=1)", "f(**a)", "f(a for a in b)", "x = (yield)", "yield", "await x",
    "if x:\n    class A: pass\n    y = 1\n", "while a:\n    try:\n        pass\n    finally:\n        pass\n    x = 1\n",
    "if a:\n    pass\nelif b is c:\n    pass\nelse:\n    pass\n", "x = [1]\ny = {1}\nz = (1, 2)\nw = 'a'\n",
    "def f(a):\n    return lambda: a\n", "for i in range(3):\n    x = [i]\nelse:\n    y = 1\n",
    "x = 1\ntry:\n    y = 2\nexcept:\n    z = 3\nprint(x)\n", "x = 1\t# tab no comentário",
    # o CPython rejeita: nós também
    "x = 1;;", ";", "if a: if b: pass", "from a import b,", "from a import ()", "@d\nx = 1", "if a:\npass", "f(,)",
    "a[]", "x = = 1", "f() = 1", "x + 1 = 2", "True = 1", "None = 1", "x: int = 1 = 2", "match x: pass",
    "print(type X)", "x = a if b", "-x = 1", "(a) (b) = 1", "1 = x", "a < b = 1", "not a = 1", "x += = 1",
    "f() += 1", "def f(a:): pass", "def f() ->: pass", "def f()->int\n    pass", "def f(a b): pass", "def f(,): pass",
    "else: pass", "elif a: pass", "if a: pass\nelse: pass\nelse: pass", "if a:\n    pass\n  else: pass",
    "x = not", "x = a <", "x = a < < b", "x = a not b", "import", "from cudra import", "from cudra", "@d",
    "@d\n", "def", "def f", "def f(", "def f():", "return return", "for x in: pass", "for in x: pass",
    "for x y: pass", "for f() in x: pass", "for -x in y: pass", "for 1 in x: pass", "while: pass", "if: pass",
    "x = (", "x = )", "x = a.", "x = a.1", "x = .a", "x = f(a b)", "x = a[1", "pass pass", "break 1",
    "x = 1 2", "x = a b", "x: = 1", "x: int, y = 1", "x = def", "x = if", "print(def)", "@\ndef f(): pass",
    "@d def f(): pass", "def f(): pass; def g(): pass", "if a: pass; if b: pass", "x = 1 if", "class",
    "x = (a for)", "x = [", "x = {", "x = a ** ", "x = ~", "x = a +", "x = +", "x = 1 +* 2",
]

PATH = {
    "lines_1MB": "x = 1\n" * 170_000,
    "add_500k": "x = " + "a+" * 500_000 + "a\n",
    "neg_100k": "x = " + "-" * 100_000 + "a\n",
    "not_100k": "x = " + "not " * 100_000 + "a\n",
    "pow_100k": "x = " + "a**" * 100_000 + "a\n",
    "call_100k": "x = a" + "()" * 100_000 + "\n",
    "attr_100k": "x = a" + ".b" * 100_000 + "\n",
    "idx_100k": "x = a" + "[0]" * 100_000 + "\n",
    "and_100k": "x = " + " and ".join(["a"] * 100_000) + "\n",
    "cmp_100k": "x = " + "<".join(["a"] * 100_000) + "\n",
    "parens_200_rep": ("x = " + "(" * 199 + "a" + ")" * 199 + "\n") * 2000,
    "dollars_1M": "$" * 1_000_000 + "\n",
    "closers_1M": ")" * 1_000_000 + "\n",
    "args_300k": "f(" + "a," * 300_000 + ")\n",
    "semis_300k": "x = 1; " * 300_000 + "\n",
    "blocks_deep_99": "".join(" " * i + "if a:\n" for i in range(99)) + " " * 99 + "pass\n",
    "many_defs": "def f(a: int) -> int:\n    return a + 1\n" * 50_000,
    "classes_100k": "class A:\n    x = 1\n" * 100_000,
    "unsupported_lines": "x = [1]\n" * 200_000,
    "decos_100k": "@d\n" * 100_000 + "def f(): pass\n",
    "elif_100k": "if a:\n    pass\n" + "elif b:\n    pass\n" * 100_000,
    "nested_calls_deep": "x = " + "f(" * 199 + "a" + ")" * 199 + "\n",
}

fails = []


def fail(kind, msg):
    fails.append(kind)
    if fails.count(kind) <= 10:
        print(f"FALHA {kind}: {msg}")


def main():
    dump = sys.argv[1]
    work = "/tmp/pa"
    os.makedirs(work, exist_ok=True)
    print("### 1. casos à mão")
    cats = {}
    for i, src in enumerate(CASES):
        for variant, s in (("", src), ("nl", src + "\n")):
            cat, detail = P.classify(s, f"{work}/c{i}{variant}.cdr", dump, "wild")
            cats[cat] = cats.get(cat, 0) + 1
            if cat in P.FAILS:
                fail(cat, f"{s!r}\n      {detail}")
    print("   ", cats)

    print("### 2. patológicos (tempo)")
    for name, src in PATH.items():
        path = f"{work}/{name}.cdr"
        with open(path, "w", newline="", encoding="utf-8") as f:
            f.write(src)
        t = time.time()
        got, errs, rc, stderr = P.run_ours(dump, path)
        dt = time.time() - t
        nerr = len(errs)
        if rc not in (0, 1) or "panicked" in stderr:
            fail("CRASH_PATOLOGICO", f"{name} rc={rc} {stderr[:200]!r}")
        elif dt > SLOW:
            fail("LENTO", f"{name} {dt:.2f}s")
        print(f"   {name:20} rc={rc} {dt:6.2f}s erros={nerr} {errs[:1]}")
        if name.endswith("_100k") and name.split("_")[0] in ("neg", "not", "pow", "call", "attr", "idx", "and", "add") and nerr != 1:
            fail("PROFUNDIDADE_ERROS", f"{name}: esperado 1 erro, obtido {nerr}")

    print("### 3. truncagem de cada caso em cada posição")
    total = 0
    for i, src in enumerate(CASES):
        for k in range(len(src)):
            total += 1
            path = f"{work}/t.cdr"
            with open(path, "w", newline="", encoding="utf-8") as f:
                f.write(src[:k])
            got, errs, rc, stderr = P.run_ours(dump, path)
            if rc not in (0, 1) or "panicked" in stderr:
                fail("CRASH_TRUNCAGEM", f"{src[:k]!r} rc={rc} {stderr[:200]!r}")
    print(f"   truncagens: {total}")
    print(f"RESUMO ataque parser: {len(fails)} falhas {dict((k, fails.count(k)) for k in sorted(set(fails)))}")
    P.STATUS.append(1 if fails else 0)


if __name__ == "__main__":
    sys.setrecursionlimit(1_000_000)
    threading.stack_size(512 * 1024 * 1024)
    th = threading.Thread(target=main)
    th.start()
    th.join()
    sys.exit(P.STATUS[0] if P.STATUS else 2)
