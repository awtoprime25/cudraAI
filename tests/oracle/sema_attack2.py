import os
import subprocess
import sys
import tempfile
import time

SHIM = "import sys\ndef arg_int(i):\n    return int(sys.argv[i])\ndef arg_float(i):\n    return float(sys.argv[i])\n"
ARGS = ["5", "2.5"]
UNSUP = ("ainda não é suportad", "ainda não são suportad")

CASES = [
    ("UNSUP", "if False:\n    f()\ndef f():\n    pass"),
    ("UNSUP", "while False:\n    f()\ndef f():\n    pass"),
    ("UNSUP", "for i in range(0):\n    f()\ndef f():\n    pass"),
    ("PYERR", "x = 1\nif x:\n    f()\ndef f():\n    pass"),
    ("OK", "def f() -> int:\n    while 2:\n        pass\nprint(1)"),
    ("STATIC", "def f() -> int:\n    while True:\n        break\nprint(1)"),
    ("ANY", "def f() -> int:\n    if True:\n        return 1\nprint(f())"),
    ("ANY", "def f() -> int:\n    while -1:\n        return 1\nprint(f())"),
    ("PYERR", "x = 1\nif x:\n    pass\nelif x:\n    y = 1\nelse:\n    y = 2\nprint(y)"),
    ("OK", "def f(a: int) -> int:\n    if a:\n        return 1\n    elif a > 1:\n        return 2\n    else:\n        return 3\nprint(f(0))"),
    ("STATIC", "def f(x: int) -> int:\n    x = 2.5\n    return 1\nprint(f(1))"),
    ("OK", "def f(x: int) -> int:\n    for x in range(3):\n        pass\n    return x\nprint(f(9))"),
    ("STATIC", "def f(n: int) -> int:\n    for i in range(n):\n        pass\n    return i\nprint(f(3))"),
    ("UNSUP", "from cudra import arg_int\ndef arg_int() -> int:\n    return 1\nprint(1)"),
    ("UNSUP", "from cudra import arg_int\narg_int = 3\nprint(arg_int)"),
    ("OKU", "import cudra\ncudra = 5\nprint(cudra)"),
    ("UNSUP", "import cudra\nprint(cudra)"),
    ("UNSUP", "import cudra\nx = cudra.arg_int\nprint(1)"),
    ("UNSUP", "print(abs)"),
    ("UNSUP", "x = print\nprint(1)"),
    ("UNSUP", "print(int)"),
    ("UNSUP", "print(print)"),
    ("OK", "def f(a: bool) -> int:\n    return int(a)\nprint(f(True))"),
    ("OK", "print(float(1) == 1, 1.0 < 2 < 3.5 > 1)"),
    ("OK", "print(int(1.5) + 1, int(-0.5))"),
    ("OK", "x = 2\nx **= 3\nprint(x)"),
    ("OK", "x = 1.5\nx //= 1\nx %= 2\nx **= 2\nprint(x)"),
    ("UNSUP", "x = True\nx += 1\nprint(x)"),
    ("STATIC", "x = True\nx = 1\nprint(x)"),
    ("STATIC", "x = 1\nx = True\nprint(x)"),
    ("OK", "def f() -> bool:\n    return 1 < 2\nprint(f())"),
    ("STATIC", "def f() -> int:\n    return True\nprint(f())"),
    ("STATIC", "def f() -> bool:\n    return 1\nprint(f())"),
    ("OK", "print(-(-9223372036854775807))"),
    ("UNSUP", "print(-9223372036854775809)"),
    ("UNSUP", "print(18446744073709551616)"),
    ("OK", "print(1e400, -1e400, 1e-400)"),
    ("OK", "print(0e0, 1_000.5e-3, 1.e5, .5e-1, 5.)"),
    ("STATIC", "def f(a: int) -> float:\n    return a / 2\nprint(f(f(1)))"),
    ("UNSUP", "def f(a: int) -> int:\n    return a\nprint(f(a=1))"),
    ("UNSUP", "x = 1\nprint(x.real)"),
    ("UNSUP", "print((1).real)"),
    ("UNSUP", "def f() -> int:\n    return 1\nprint(f.__name__)"),
    ("OK", "print(abs(-9223372036854775807))"),
    ("UNSUP", "def f(n: int):\n    if n > 0:\n        f(n - 1)\nf(3)"),
    ("OK", "def f(n: int) -> None:\n    if n > 0:\n        f(n - 1)\nf(3)"),
    ("OK", "def a() -> int:\n    return b()\ndef b() -> int:\n    return 1\nx = a()\nprint(x)"),
    ("PYERR", "def a() -> int:\n    return b()\nx = 1\nif x:\n    print(a())\ndef b() -> int:\n    return 1"),
    ("STATIC", "i = 0\nwhile True:\n    if i > 2:\n        break\n    y = i\n    i += 1\nprint(y)"),
    ("OK", "while True:\n    y = 1\n    if y:\n        break\nprint(y)"),
    ("OK", "n = 0\nwhile True:\n    n += 1\n    if n < 3:\n        continue\n    z = n\n    break\nprint(z)"),
    ("STATIC", "for i in range(3):\n    q = i\n    break\nprint(q)"),
    ("STATIC", "x = 1\nwhile x:\n    w = 1\n    break\nprint(w)"),
    ("OK", "while True:\n    while True:\n        a = 1\n        break\n    b = a\n    break\nprint(a, b)"),
    ("PYERR", "while True:\n    while True:\n        break\n        a = 1\n    b = 2\n    break\nprint(b, a)"),
    ("OK", "k = 0\nwhile True:\n    k += 1\n    if k == 1:\n        continue\n    elif k == 2:\n        m = 5\n        break\n    else:\n        m = 6\n        break\nprint(m)"),
    ("PYERR", "def f() -> int:\n    y = 1\n    return y\nprint(y)"),
    ("UNSUP", "x: list[int] = 1\nprint(x)"),
    ("OKU", "x: None = None\nprint(x)"),
    ("STATIC", "x: bool = 1\nprint(x)"),
    ("STATIC", "x: int = True\nprint(x)"),
    ("UNSUP", "x: 'int' = 1\nprint(x)"),
    ("UNSUP", "def f() -> 'int':\n    return 1\nprint(f())"),
    ("STATIC", "def f(a: int) -> int:\n    a += 1.5\n    return 1\nprint(f(1))"),
    ("OK", "def f(a: float) -> float:\n    a += 1\n    return a\nprint(f(1.0))"),
    ("OK", "x = 5\nx = -x\nx = ~x\nx = +x\nprint(x)"),
    ("OK", "print(not not 5)"),
    ("UNSUP", "print(not None)"),
    ("UNSUP", "print(1 < 2 == True)"),
    ("OK", "print((1 < 2) == (2 < 3))"),
    ("UNSUP", "print((1 < 2) < (2 < 3))"),
    ("OK", "print(2 < 3 and 3 < 4 or not 5 > 6)"),
    ("OK", "print(True and 1 < 2)"),
    ("STATIC", "x = 1 < 2\nx = 5\nprint(x)"),
    ("OKU", "ação = 1\nprint(ação)"),
    ("OKU", "\ufb01 = 1\nprint(fi)"),
    ("OKU", "\U0001d431 = 1\nprint(x)"),
    ("OKU", "x = 1\nprint(\U0001d431)"),
    ("OK", "def f(f: int) -> int:\n    return f\nprint(f(2))"),
    ("OK", "def f() -> int:\n    f = 1\n    return f\nprint(f())"),
    ("OK", "def g() -> int:\n    return 1\ndef f() -> int:\n    g = 2\n    return g\nprint(f(), g())"),
    ("PYERR", "def f() -> int:\n    x = g()\n    g = 1\n    return x\ndef g() -> int:\n    return 1\nprint(f())"),
    ("UNSUP", "def f() -> int:\n    print = 1\n    return print\nprint(f())"),
    ("OKU", "import cudra\ndef f() -> int:\n    cudra = 1\n    return cudra\nprint(f())"),
    ("OK", "import cudra\ndef f() -> int:\n    return cudra.arg_int(1)\nprint(f())"),
    ("OKU", "from cudra import arg_int\ndef f() -> int:\n    arg_int = 2\n    return arg_int\nprint(f())"),
    ("UNSUP", "from cudra import *\nprint(arg_int(1))"),
    ("UNSUP", "import cudra as c\nprint(c.arg_int(1))"),
    ("UNSUP", "from cudra import arg_int as a\nprint(a(1))"),
    ("PYERR", "import cudra.x"),
    ("UNSUP", "import cudra, math\nprint(1)"),
    ("PYERR", "print(arg_float(1) + arg_int(1))"),
    ("UNSUP", "from cudra import arg_int\nprint(arg_int(True))"),
    ("PYERR", "from cudra import arg_int\nprint(arg_int(1.0))"),
    ("PYERR", "from cudra import arg_int\nprint(arg_int())"),
    ("PYERR", "from cudra import arg_int\nprint(arg_int(1, 2))"),
    ("OK", "print(int(2.5, ))"),
    ("PYERR", "print(abs(1, 2))"),
    ("OK", "print(float(True) + int(False))"),
    ("UNSUP", "print(bool(1) + 1)"),
    ("PYERR", "print(int(None))"),
    ("PYERR", "print(float(None))"),
    ("PYERR", "def f():\n    pass\nprint(int(f()))"),
    ("OK", "def f():\n    pass\nx = f()\nx = f()\nprint(x)"),
    ("STATIC", "x = None\nx = 1\nprint(x)"),
    ("OK", "def f(a: int) -> int:\n    return a\ndef g():\n    return f(1)\nprint(g())"),
    ("OK", "def g():\n    return h()\ndef h():\n    return 1\nprint(g())"),
    ("OK", "def g():\n    return h() + 0.5\ndef h():\n    return k()\ndef k():\n    return 1\nprint(g())"),
    ("OK", "def g():\n    x = h()\n    return x\ndef h():\n    return 2.5\nprint(g() + 1)"),
    ("OK", "def g():\n    if h():\n        return 1\n    return 2\ndef h():\n    return 1 < 2\nprint(g())"),
    ("STATIC", "def g():\n    return h() + 1\ndef h():\n    return 2.5\nx = 1\nx = g()\nprint(x)"),
    ("UNSUP", "def f():\n    return g()\ndef g():\n    return f()\nprint(1)"),
    ("OKU", "def f():\n    return g()\ndef g() -> int:\n    return f()\nprint(1)"),
    ("OK", "def f() -> int:\n    return f() + 1\nprint(1)"),
    ("OK", "def f(n: int) -> int:\n    return (n)\nprint(f(3))"),
    ("OK", "print(2 ** 63 - 1 if False else 1)" if False else "print(2 ** 62)"),
    ("UNSUP", "x = 1\nx = x.bit_length()\nprint(x)"),
    ("OK", "print(1 < 2 < 3 < 4 < 5 < 6.5)"),
    ("OK", "if 1:\n    pass\nelse:\n    pass"),
    ("OK", "while False:\n    pass\nprint(1)"),
    ("STATIC", "for i in range(3):\n    for i in range(2):\n        pass\nprint(i)"),
    ("OK", "for i in range(5, 0, -1):\n    print(i)"),
    ("UNSUP", "print(range(3))"),
    ("PYERR", "for i in range(3.0 // 1):\n    pass"),
    ("OK", "from cudra import arg_int\nfor i in range(arg_int(1)):\n    print(i)"),
    ("OK", "for i in range(-5):\n    x = 1\nprint(1)"),
    ("STATIC", "def f(a: int, b: int, c: int) -> int:\n    return a\nprint(f(1, 2.0, 3))"),
    ("STATIC", "def f(a: int):\n    pass\nf(print())"),
    ("STATIC", "def f():\n    pass\ndef g(a: int):\n    pass\ng(f())"),
    ("OK", "print(print(1), print(2))"),
    ("OK", "x = 1\nx = (x)\nprint(x)"),
    ("UNSUP", "x: int\nx = 1\nprint(x)"),
    ("OKU", "(x) = 1\nprint(x)"),
    ("UNSUP", "x, y = 1, 2\nprint(x)"),
    ("UNSUP", "x = y = 1\nprint(x)"),
    ("OK", "x = 1;\nprint(x);"),
    ("UNSUP", "assert True\nprint(1)"),
    ("UNSUP", "x = 1\ndel x\nprint(1)"),
    ("UNSUP", "print(1 in range(3))"),
    ("OK", "print(0x10, 0o10, 0b10, 1_0, 00)"),
    ("UNSUP", "print(1j)"),
    ("STATIC", "def f() -> int: pass\nprint(1)"),
    ("OK", ""),
    ("OK", "# só um comentário"),
    ("OK", "pass"),
    ("OK", "\n\n\n"),
    ("OK", "def f() -> int:\n    return 1\n\n\n\nprint(f())\n\n"),
    ("OK", "x = 0\nif x == 0:\n    x = 1\nelif x == 1:\n    x = 2\nprint(x)"),
    ("OK", "def f(a: int, b: int) -> int:\n    return a - b\nprint(f(b=1, a=2) if False else f(2, 1))" if False else "def f(a: int, b: int) -> int:\n    return a - b\nprint(f(2, 1))"),
    ("OK", "def p(x: int) -> int:\n    print(x)\n    return x\nprint(p(1) < p(2) < p(0) < p(3))"),
    ("OK", "def p(x: int) -> bool:\n    print(x)\n    return x > 1\nprint(p(1) and p(2), p(3) or p(4))"),
    ("OK", "x = 10\nwhile x > 0:\n    x -= 3\n    if x == 4:\n        continue\n    if x < 2:\n        break\nprint(x)"),
    ("OK", "def f(a: int) -> int:\n    for i in range(10):\n        if i == a:\n            return i\n    return -1\nprint(f(3), f(20))"),
    ("OK", "def f(a: int) -> int:\n    while True:\n        for i in range(3):\n            if i == a:\n                return i\n        a -= 1\nprint(f(5))"),
    ("STATIC", "def f(a: int) -> int:\n    for i in range(3):\n        while True:\n            return 1\nprint(f(5))"),
    ("OK", "def f() -> int:\n    while True:\n        while True:\n            return 1\nprint(f())"),
    ("OK", "def f() -> int:\n    while True:\n        while True:\n            break\n        return 2\nprint(f())"),
    ("STATIC", "def f() -> int:\n    while True:\n        while True:\n            break\n        break\nprint(f())"),
    ("OK", "def f() -> int:\n    while True:\n        pass\nprint(1)"),
    ("OK", "def f() -> int:\n    while True:\n        continue\nprint(1)"),
    ("OK", "def f(a: int) -> int:\n    while True:\n        if a:\n            continue\n        return 1\nprint(f(0))"),
]


def run(cmd, timeout, env=None):
    try:
        p = subprocess.run(cmd, capture_output=True, timeout=timeout, env=env)
        return p.returncode, p.stdout.decode("utf-8", "replace"), p.stderr.decode("utf-8", "replace")
    except subprocess.TimeoutExpired:
        return None, "", "TIMEOUT"


def perf_cases():
    n = 10_000
    return {
        "inferencia_rev_10k": "".join(f"def f{i}():\n    return f{i + 1}() + 1\n" for i in range(n))
        + f"def f{n}():\n    return 1\nprint(f0())\n",
        "inferencia_rev_desc": f"def f{n}():\n    return 1\n"
        + "".join(f"def f{i}():\n    return f{i + 1}() + 1\n" for i in range(n - 1, -1, -1)) + "print(f0())\n",
        "elif_3000": "x = 5\n" + "if x == 0:\n    y = 0\n" + "".join(f"elif x == {i}:\n    y = {i}\n" for i in range(1, 3000))
        + "else:\n    y = -1\nprint(y)\n",
        "params_500": "def f(" + ", ".join(f"a{i}: int" for i in range(500)) + ") -> int:\n    return a0 + a499\n"
        + "print(f(" + ", ".join(str(i) for i in range(500)) + "))\n",
        "ident_10k": f"{'v' * 10_000} = 1\nprint({'v' * 10_000})\n",
        "breaks_2000": "i = 0\nwhile True:\n" + "".join(f"    if i == {k}:\n        z{k} = 1\n        break\n" for k in range(2000))
        + "    i += 1\nprint(i)\n",
        "chain_cmp_2000": "x = 1\nprint(" + " < ".join(["x"] * 2000) + ")\n",
        "and_5000": "x = True\nprint(" + " and ".join(["x"] * 5000) + ")\n",
        "erros_10k": "".join(f"y{i} = 1\ny{i} = 2.5\n" for i in range(10_000)),
        "nested_ifs_95": "x = 1\n" + "".join("    " * d + "if x:\n" for d in range(95)) + "    " * 95 + "y = 1\nprint(x)\n",
    }


def main():
    cudrac = sys.argv[1]
    work = tempfile.mkdtemp(prefix="sema_attack2_")
    shim = os.path.join(work, "shim")
    os.makedirs(shim, exist_ok=True)
    open(os.path.join(shim, "cudra.py"), "w").write(SHIM)
    env = dict(os.environ, PYTHONPATH=shim)
    fails, counts = [], {}
    for i, (exp, src) in enumerate(CASES):
        path = os.path.join(work, f"c{i}.cdr")
        open(path, "w", encoding="utf-8").write(src + "\n")
        prc, _, perr = run(["python3", path] + ARGS, 10, env)
        py_ok = prc == 0
        if exp != "ANY" and (exp == "PYERR") == py_ok:
            fails.append(("CASO_MAL_ROTULADO", exp, src, perr[-300:]))
            continue
        counts[exp] = counts.get(exp, 0) + 1
        if cudrac == "-":
            continue
        rc, out, err = run([cudrac, "check", path], 20)
        if rc is None or rc not in (0, 1) or "panicked" in err or "erro interno" in err:
            fails.append(("CRASH", exp, src, f"rc={rc} {err[-400:]}"))
        elif exp == "ANY":
            pass
        elif exp == "OK" and rc != 0:
            fails.append(("REJEITOU_VALIDO", exp, src, err[-600:]))
        elif exp in ("STATIC", "UNSUP", "PYERR") and rc == 0:
            fails.append(("ACEITOU_INVALIDO", exp, src, ""))
        elif exp in ("UNSUP", "OKU") and rc == 1 and not any(u in err for u in UNSUP):
            fails.append(("SEM_AINDA_NAO", exp, src, err[-600:]))
        if rc == 0 and exp in ("OK", "OKU", "ANY"):
            rc2, out2, err2 = run([cudrac, "build", path, "--emit=tir"], 20)
            if rc2 != 0 or not out2.strip() or "panicked" in err2:
                fails.append(("EMIT_TIR_FALHOU", exp, src, f"rc={rc2} {err2[-400:]}"))
    if cudrac != "-":
        for name, src in perf_cases().items():
            path = os.path.join(work, f"perf_{name}.cdr")
            open(path, "w").write(src)
            t = time.time()
            rc, _, err = run([cudrac, "check", path], 60)
            dt = time.time() - t
            print(f"   {name:22s} rc={rc} {dt:5.2f}s linhas_stderr={err.count(chr(10))}")
            if rc is None or rc not in (0, 1) or "panicked" in err or "erro interno" in err:
                fails.append(("CRASH_PERF", name, "", f"rc={rc} {err[-300:]}"))
            elif dt > 2.0:
                fails.append(("LENTO", name, "", f"{dt:.2f}s"))
            elif name != "erros_10k" and rc != 0:
                fails.append(("PERF_REJEITADO", name, "", err[-400:]))
    for kind, exp, src, info in fails[:80]:
        print(f"FALHA {kind} [{exp}]\n{src}\n  -> {info}\n")
    cats = {}
    for f in fails:
        cats[f[0]] = cats.get(f[0], 0) + 1
    print("casos:", counts)
    print(f"RESUMO ataque sema 2: {len(fails)} falhas {cats}")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
