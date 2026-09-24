"""Ataque à sema: casos à mão com o veredicto esperado, validados contra o CPython.

uso: python3 sema_attack.py <cudrac|-> [--selftest]
  OK     o CPython corre sem erro e nós temos de aceitar
  UNSUP  o CPython corre sem erro e nós rejeitamos com "ainda não é/são suportad"
  STATIC o CPython corre sem erro e nós rejeitamos por tipagem estática (qualquer mensagem)
  PYERR  o CPython falha (NameError, TypeError, ...) e nós temos de rejeitar
"""
import os, subprocess, sys, tempfile, time

SHIM = "import sys\ndef arg_int(i):\n    return int(sys.argv[i])\ndef arg_float(i):\n    return float(sys.argv[i])\n"
ARGS = ["5", "2.5"]
UNSUP = ("ainda não é suportad", "ainda não são suportad")

CASES = [
    ("OK", "x = 1\nx = 2\nprint(x)"),
    ("STATIC", "x = 1\nx = 2.5\nprint(x)"),
    ("STATIC", "x: float = 1\nprint(x)"),
    ("OK", "x: float = 1.0\nprint(x)"),
    ("STATIC", "def f(a: float) -> float:\n    return a\nprint(f(1))"),
    ("STATIC", "def f() -> float:\n    return 1\nprint(f())"),
    ("STATIC", "x = 1\nx += 0.5\nprint(x)"),
    ("OK", "y = 1.0\ny += 1\nprint(y)"),
    ("STATIC", "x = 4\nx /= 2\nprint(x)"),
    ("OK", "x = 4\nx //= 2\nprint(x)"),
    ("STATIC", "x = 4\nx //= 2.0\nprint(x)"),
    ("STATIC", "def f(a: int):\n    if a > 0:\n        return 1\n    return 1.5\nprint(f(1))"),
    ("STATIC", "def f(a: int):\n    if a > 0:\n        return 1\nprint(f(1))"),
    ("STATIC", "def f(a: int) -> int:\n    if a > 0:\n        return 1\nprint(f(1))"),
    ("OK", "def f(a: int) -> int:\n    if a > 0:\n        return 1\n    else:\n        return 2\nprint(f(1))"),
    ("OK", "def f(a: int) -> int:\n    while True:\n        if a > 3:\n            return a\n        a += 1\nprint(f(1))"),
    ("OK", "def f(a: int) -> int:\n    while 1:\n        return a\nprint(f(1))"),
    ("STATIC", "def f(a: int) -> int:\n    while a:\n        return a\nprint(f(1))"),
    ("OK", "def f(a: int) -> int:\n    while True:\n        a += 1\n        if a > 5:\n            break\n    return a\nprint(f(1))"),
    ("OK", "i = 0\nwhile True:\n    x = i\n    i += 1\n    if i > 3:\n        break\nprint(x)"),
    ("STATIC", "i = 0\nwhile True:\n    i += 1\n    if i > 3:\n        break\n    x = i\nprint(x)"),
    ("OK", "def f(a: int):\n    return\nprint(f(1))"),
    ("OK", "def f(a: int):\n    return None\nprint(f(1))"),
    ("OK", "def f(a: int):\n    if a:\n        return\n    return None\nprint(f(1))"),
    ("OK", "def f(a: int) -> None:\n    pass\nprint(f(1))"),
    ("STATIC", "def f(a: int) -> None:\n    return 1\nf(1)"),
    ("UNSUP", "def f(n: int):\n    if n <= 0:\n        return 0\n    return f(n - 1)\nprint(f(3))"),
    ("OK", "def f(n: int) -> int:\n    if n <= 0:\n        return 0\n    return f(n - 1)\nprint(f(3))"),
    ("UNSUP", "def a(n: int) -> int:\n    if n <= 0:\n        return 0\n    return b(n - 1)\ndef b(n: int):\n    return a(n)\nprint(a(3))"),
    ("OK", "def a(n: int) -> int:\n    if n <= 0:\n        return 0\n    return b(n - 1)\ndef b(n: int) -> int:\n    return a(n)\nprint(a(3))"),
    ("OK", "def b(n: int) -> int:\n    return a(n)\ndef a(n: int) -> int:\n    if n <= 0:\n        return 0\n    return b(n - 1)\nprint(a(3))"),
    ("OK", "def g() -> int:\n    return h()\ndef h() -> int:\n    return 1\nprint(g())"),
    ("PYERR", "def g() -> int:\n    return h()\nprint(g())\ndef h() -> int:\n    return 1"),
    ("PYERR", "def g() -> int:\n    return k()\ndef k() -> int:\n    return h()\nprint(g())\ndef h() -> int:\n    return 1"),
    ("OK", "def g() -> int:\n    return k()\ndef k() -> int:\n    return h()\ndef h() -> int:\n    return 1\nprint(g())"),
    ("PYERR", "f()\ndef f():\n    pass"),
    ("PYERR", "def f() -> int:\n    return arg_int(1)\nprint(f())\nfrom cudra import arg_int"),
    ("OK", "def f() -> int:\n    return arg_int(1)\nfrom cudra import arg_int\nprint(f())"),
    ("PYERR", "print(arg_int(1))\nfrom cudra import arg_int"),
    ("OK", "import cudra\nprint(cudra.arg_int(1))"),
    ("OK", "import cudra\nprint(cudra.arg_float(2))"),
    ("PYERR", "print(cudra.arg_int(1))"),
    ("PYERR", "import cudra\nprint(cudra.arg_str(1))"),
    ("OK", "from cudra import arg_int\nfrom cudra import arg_int\nprint(arg_int(1))"),
    ("OK", "from cudra import arg_int, arg_float\nprint(arg_int(1), arg_float(2))"),
    ("PYERR", "from cudra import arg_str"),
    ("UNSUP", "import math\nprint(1)"),
    ("UNSUP", "from math import floor\nprint(1)"),
    ("OK", "x = 1\nif x > 0:\n    y = 1\nelse:\n    y = 2\nprint(y)"),
    ("OK", "x = 1\nif x > 0:\n    y = 1\nelif x < 0:\n    y = 2\nelse:\n    y = 3\nprint(y)"),
    ("STATIC", "x = 1\nif x > 0:\n    y = 1\nelif x < 0:\n    y = 2\nprint(y)"),
    ("STATIC", "if True:\n    y = 1\nprint(y)"),
    ("STATIC", "for i in range(3):\n    pass\nprint(i)"),
    ("OK", "i = 7\nfor i in range(3):\n    pass\nprint(i)"),
    ("STATIC", "i = 7.0\nfor i in range(3):\n    pass\nprint(i)"),
    ("OK", "for i in range(3):\n    i = i + 100\n    print(i)"),
    ("STATIC", "n = 0\nwhile n < 3:\n    z = n\n    n += 1\nprint(z)"),
    ("OK", "n = 0\nfor k in range(3):\n    if k == 1:\n        continue\n    n += k\nprint(n)"),
    ("STATIC", "for k in range(3):\n    if k == 1:\n        break\n    w = k\nprint(w)"),
    ("STATIC", "def f() -> int:\n    return m\nm = 1\nprint(f())"),
    ("STATIC", "m = 1\ndef f() -> int:\n    return m\nprint(f())"),
    ("PYERR", "m = 1\ndef f() -> int:\n    x = m\n    m = 2\n    return x\nprint(f())"),
    ("OK", "m = 1\ndef f() -> int:\n    m = 2\n    return m\nprint(f(), m)"),
    ("OK", "def f(m: int) -> int:\n    return m\nm = 5\nprint(f(m))"),
    ("PYERR", "print(zz)"),
    ("PYERR", "zz = zz + 1"),
    ("PYERR", "x += 1"),
    ("OK", "sum = 0\nfor i in range(4):\n    sum += i\nprint(sum)"),
    ("OK", "max = 3\nmin = 1\nlen = 2\nid = 4\nprint(max, min, len, id)"),
    ("UNSUP", "print = 3"),
    ("UNSUP", "abs = 3\nprint(abs)"),
    ("UNSUP", "def range(a: int) -> int:\n    return a\nprint(range(3))"),
    ("UNSUP", "def f(int: int) -> int:\n    return int\nprint(f(3))"),
    ("UNSUP", "for float in range(3):\n    pass\nprint(1)"),
    ("UNSUP", "print(max(1, 2))"),
    ("UNSUP", "print(round(2.5))"),
    ("UNSUP", "print(len(\"ab\"))"),
    ("PYERR", "g = 1\ng()"),
    ("PYERR", "def h(g: int) -> int:\n    return g()\nprint(h(1))"),
    ("OK", "def g() -> int:\n    return 1\ndef h(g: int) -> int:\n    return g\nprint(h(5), g())"),
    ("UNSUP", "def f() -> int:\n    return 1\nx = f\nprint(1)"),
    ("UNSUP", "def f() -> int:\n    return 1\nprint(f)"),
    ("UNSUP", "def f() -> int:\n    return 1\nf"),
    ("UNSUP", "def f() -> int:\n    return 1\nf = 2\nprint(f)"),
    ("UNSUP", "f = 2\ndef f() -> int:\n    return 1\nprint(f())"),
    ("UNSUP", "def f() -> int:\n    return 1\ndef f() -> int:\n    return 2\nprint(f())"),
    ("UNSUP", "print(True + 1)"),
    ("UNSUP", "print(-True)"),
    ("UNSUP", "print(~True)"),
    ("UNSUP", "print(abs(True))"),
    ("UNSUP", "for i in range(True):\n    print(i)"),
    ("UNSUP", "print(True < False)"),
    ("UNSUP", "print(1 == True)"),
    ("UNSUP", "print(1.0 != False)"),
    ("OK", "print(True == False, True != True)"),
    ("OK", "print(int(True), float(False), bool(3), bool(0.0), bool(-0.0))"),
    ("OK", "print(int(), float(), bool())"),
    ("PYERR", "print(abs())"),
    ("PYERR", "print(int(5, 10))"),
    ("OK", "print(not 5, not 0, not 0.0, not 1.5, not True)"),
    ("OK", "x = 3\nif x:\n    print(1)\nwhile x:\n    x -= 1\nprint(x)"),
    ("OK", "x = 0.5\nif x:\n    print(1)"),
    ("UNSUP", "print(1 and 2)"),
    ("UNSUP", "print(True and 2)"),
    ("OK", "print(True and False or True)"),
    ("PYERR", "def f():\n    pass\nprint(f() + 1)"),
    ("UNSUP", "def f():\n    pass\nif f():\n    print(1)"),
    ("UNSUP", "def f():\n    pass\nprint(not f())"),
    ("UNSUP", "print(bool(None))"),
    ("UNSUP", "x = None\nprint(x == None)"),
    ("OK", "x = print()\nprint(x)"),
    ("OK", "def f():\n    print(1)\nx = f()\nprint(x)"),
    ("STATIC", "x = print()\nx = 1\nprint(x)"),
    ("OK", "def f(a: int):\n    print(a)\ndef g(a: int):\n    print(a)\nprint(f(1), g(2))"),
    ("OK", "print(-9223372036854775808)"),
    ("OK", "print(-(9223372036854775808))"),
    ("OK", "print(-0x8000000000000000)"),
    ("UNSUP", "print(9223372036854775808)"),
    ("UNSUP", "print(0xFFFFFFFFFFFFFFFF)"),
    ("UNSUP", "x = 9223372036854775808 - 1\nprint(x)"),
    ("OK", "print(9223372036854775807, -9223372036854775807)"),
    ("UNSUP", "x = range(3)\nprint(1)"),
    ("PYERR", "for x in 5:\n    pass"),
    ("UNSUP", "x = 1\nfor i in (x, x):\n    print(i)"),
    ("PYERR", "for x in range(1, 2, 3, 4):\n    pass"),
    ("PYERR", "for x in range():\n    pass"),
    ("PYERR", "for x in range(1.5):\n    pass"),
    ("OK", "for x in range(-9223372036854775808, 9223372036854775807, 9223372036854775807):\n    print(x)"),
    ("UNSUP", "x: str = 1\nprint(x)"),
    ("UNSUP", "def f(a: list) -> int:\n    return 1\nprint(1)"),
    ("OK", "x: int = 1\nx: int = 2\nprint(x)"),
    ("STATIC", "x: int = 1\nx: float = 2.0\nprint(x)"),
    ("OK", "def f(a: int, b: float, c: bool) -> float:\n    if c:\n        return a + b\n    return b\nprint(f(1, 2.5, True))"),
    ("PYERR", "def f(a: int) -> int:\n    return a\nprint(f(1, 2))"),
    ("PYERR", "def f(a: int) -> int:\n    return a\nprint(f())"),
    ("STATIC", "def f(a: int) -> int:\n    return a\nprint(f(True))"),
    ("STATIC", "def f(a: int) -> int:\n    return a\nprint(f(1.0))"),
    ("OK", "print(1 < 2.5 < 3, 2**53 + 1 == 9007199254740992.0)"),
    ("OK", "print(1 / 2, 7 // 2.0, 7 % 2.5, 2 ** 0.5, 2.0 ** 2)"),
    ("PYERR", "print(1.5 & 1)"),
    ("PYERR", "print(1.5 << 1)"),
    ("OK", "print(+5, -5, ~5, -2 ** 2, (-2) ** 2)"),
    ("OK", "x = 1\nx <<= 3\nx >>= 1\nx &= 7\nx |= 8\nx ^= 1\nx **= 2\nx %= 5\nprint(x)"),
    ("UNSUP", "@staticmethod\ndef f() -> int:\n    return 1\nprint(f())"),
    ("PYERR", "x = 1\nx[0] = 2"),
    ("PYERR", "return 1"),
    ("PYERR", "break"),
    ("PYERR", "if True:\n    continue"),
    ("PYERR", "def f():\n    break"),
    ("OK", "def f() -> int:\n    for i in range(3):\n        if i == 1:\n            break\n    return 1\nprint(f())"),
    ("UNSUP", "def f(a) -> int:\n    return 1\nprint(f(1))"),
    ("PYERR", "def f(a: int, a: int) -> int:\n    return a\nprint(f(1, 2))"),
    ("UNSUP", "def f() -> int:\n    from cudra import arg_int\n    return arg_int(1)\nprint(f())"),
    ("UNSUP", "def f() -> int:\n    import cudra\n    return 1\nprint(f())"),
    ("OK", "x = 1\nif x > 0: y = 2\nelse: y = 3\nprint(y)"),
    ("OK", "for i in range(2): print(i)"),
    ("OK", "def f(n: int) -> int: return n + 1\nprint(f(1))"),
    ("OK", "x = 1; y = 2; print(x + y)"),
    ("OK", "def f() -> int:\n    return 1\n    x = 2.5\nprint(f())"),
    ("STATIC", "def f() -> int:\n    return 1\n    x = 1\n    x = 2.5\nprint(f())"),
    ("OK", "def f(n: int) -> int:\n    if n > 0:\n        return 1\n    else:\n        return 2\n    print(3)\nprint(f(1))"),
    ("OK", "def f() -> float:\n    return 1.0\ndef g() -> float:\n    return f() * 2\nprint(g())"),
    ("OK", "def f(x: float) -> float:\n    return x\nprint(f(float(3)))"),
    ("OK", "print(abs(-3), abs(-2.5), abs(-0.0))"),
    ("OK", "print(float(1), int(2.7), int(-2.7))"),
    ("OK", "x = 1\nprint(x, 1.5, True, None)"),
    ("OK", "print()"),
    ("OK", "def main() -> int:\n    return 1\nprint(main())"),
    ("OK", "def cdr_x() -> int:\n    return 1\nprint(cdr_x())"),
    ("OK", "def f(print_: int) -> int:\n    return print_\nprint(f(1))"),
    ("OK", "x = 1\nx = x\nprint(x)"),
    ("OK", "a = 0\nb = a\nprint(a, b)"),
]


def run(cmd, timeout, stdin=None):
    try:
        p = subprocess.run(cmd, capture_output=True, timeout=timeout)
        return p.returncode, p.stdout.decode("utf-8", "replace"), p.stderr.decode("utf-8", "replace")
    except subprocess.TimeoutExpired:
        return None, "", "TIMEOUT"


def main():
    cudrac = sys.argv[1]
    work = tempfile.mkdtemp(prefix="sema_attack_")
    shim = os.path.join(work, "shim")
    os.makedirs(shim, exist_ok=True)
    open(os.path.join(shim, "cudra.py"), "w").write(SHIM)
    env = dict(os.environ, PYTHONPATH=shim)
    fails = []
    counts = {}
    for i, (exp, src) in enumerate(CASES):
        path = os.path.join(work, f"c{i}.cdr")
        open(path, "w").write(src + "\n")
        p = subprocess.run(["python3", path] + ARGS, capture_output=True, env=env, timeout=10)
        py_ok = p.returncode == 0
        if (exp == "PYERR") == py_ok:
            fails.append(("CASO_MAL_ROTULADO", exp, src, p.stderr.decode()[-300:]))
            continue
        counts[exp] = counts.get(exp, 0) + 1
        if cudrac == "-":
            continue
        rc, out, err = run([cudrac, "check", path], 20)
        if rc is None or rc not in (0, 1) or "panicked" in err or "erro interno" in err:
            fails.append(("CRASH", exp, src, f"rc={rc} {err[-400:]}"))
        elif exp == "OK" and rc != 0:
            fails.append(("REJEITOU_VALIDO", exp, src, err[-600:]))
        elif exp != "OK" and rc == 0:
            fails.append(("ACEITOU_INVALIDO", exp, src, ""))
        elif exp == "UNSUP" and not any(u in err for u in UNSUP):
            fails.append(("SEM_AINDA_NAO", exp, src, err[-600:]))
        elif exp == "OK":
            rc2, out2, err2 = run([cudrac, "build", path, "--emit=tir"], 20)
            if rc2 != 0 or not out2.strip():
                fails.append(("EMIT_TIR_FALHOU", exp, src, f"rc={rc2} {err2[-400:]}"))

    if cudrac != "-":
        perf = {
            "100k_linhas": "x = 0\n" + "x = x + 1\n" * 100_000 + "print(x)\n",
            "cadeia_10k": "".join(f"def f{i}(n: int) -> int:\n    return f{i + 1}(n)\n" for i in range(10_000))
            + "def f10000(n: int) -> int:\n    return n\nprint(f0(1))\n",
            "cadeia_100k_rev": "def f100000(n: int) -> int:\n    return n\n"
            + "".join(f"def f{i}(n: int) -> int:\n    return f{i + 1}(n)\n" for i in range(99_999, -1, -1))
            + "print(f0(1))\n",
            "ciclo_100k": "".join(f"def f{i}(n: int) -> int:\n    return f{(i + 1) % 100_000}(n)\n" for i in range(100_000))
            + "print(1)\n",
            "chamadas_topo_50k": "def f(n: int) -> int:\n    return n\n" + "print(f(1))\n" * 50_000,
            "chamadas_topo_antes": "".join(f"print(g{i}(1))\n" for i in range(20_000))
            + "".join(f"def g{i}(n: int) -> int:\n    return n\n" for i in range(20_000)),
            "vars_ifs": "".join(f"v{i} = {i}\n" for i in range(3000))
            + "".join(f"if v{i} > 5:\n    v{i} = 1\n" for i in range(3000)) + "print(v0)\n",
            "talvez_5k": "c = 1\n" + "".join(f"if c > 0:\n    w{i} = 1\n" for i in range(5000))
            + "".join(f"print(w{i})\n" for i in range(5000)),
            "aninhado_999": "x = 1\nprint(" + "-" * 900 + "x)\n",
            "while_aninhado": "i = 0\n" + "".join("    " * d + "while i < 1:\n" for d in range(90)) + "    " * 90 + "i += 1\nprint(i)\n",
        }
        for name, src in perf.items():
            path = os.path.join(work, f"perf_{name}.cdr")
            open(path, "w").write(src)
            t = time.time()
            rc, out, err = run([cudrac, "check", path], 60)
            dt = time.time() - t
            ndiag = err.count("erro")
            print(f"   {name:22s} rc={rc} {dt:5.2f}s erros~{ndiag}")
            if rc is None or rc not in (0, 1) or "panicked" in err or "erro interno" in err:
                fails.append(("CRASH_PERF", name, "", f"rc={rc} {err[-300:]}"))
            elif dt > 2.0:
                fails.append(("LENTO", name, "", f"{dt:.2f}s"))
            if name == "chamadas_topo_antes" and ndiag > 20_000 * 2:
                fails.append(("CASCATA", name, "", f"{ndiag}"))

    for kind, exp, src, info in fails[:60]:
        print(f"FALHA {kind} [{exp}]\n{src}\n  -> {info}\n")
    cats = {}
    for f in fails:
        cats[f[0]] = cats.get(f[0], 0) + 1
    print("casos:", counts)
    print(f"RESUMO ataque sema: {len(fails)} falhas {cats}")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
