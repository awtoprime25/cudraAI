"""Ataque ao codegen: programas à mão executados em debug e release, comparados com o CPython.

uso: python3 run_attack.py <cudrac|->
  (src, args)                       stdout + exit iguais ao python3 nos dois modos
  (src, args, {"debug": (out, rc), "release": (out, rc)})   divergência documentada
  "-" corre só o CPython e confirma que os casos sem divergência correm e que os divergentes
  diferem de facto do Python (senão a divergência está mal descrita).
"""
import os, subprocess, sys, tempfile

SHIM = "import sys\ndef arg_int(i):\n    return int(sys.argv[i])\ndef arg_float(i):\n    return float(sys.argv[i])\n"
I64_MIN = "-9223372036854775808"
I64_MAX = "9223372036854775807"

def ovf(out_before, wrapped_out):
    return {"debug": (out_before, 1), "release": (wrapped_out, 0)}

CASES = [
    ("a = -7\nwhile a <= 7:\n    b = -3\n    while b <= 3:\n        if b != 0:\n            print(a, b, a // b, a % b)\n        b += 1\n    a += 1", []),
    ("a = -7.5\nwhile a <= 7.5:\n    b = -2.5\n    while b <= 2.5:\n        if b != 0.0:\n            print(a, b, a // b, a % b, a / b)\n        b += 1.25\n    a += 1.5", []),
    ("print(1 // 0.1, 1 % 0.1, -4.0 % 2.0, 4.0 % -2.0, -0.0 // 1.0, 0.0 // -1.0, -7 // 2.0)", []),
    ("inf = 1e308 * 10\nprint(inf // 1, inf % 1, 1 % inf, -1 % inf, -1 // inf, 1e308 // 1e-308, -1e-300 // 1e300)", []),
    (f"x = {I64_MIN}\nprint(x % -1, x // 1, x % 1, x % {I64_MAX}, x // {I64_MAX})", []),
    (f"x = {I64_MIN}\nprint(1)\nprint(x // -1)", [], ovf("1\n", "1\n-9223372036854775808\n")),
    (f"x = {I64_MAX}\nprint(x / 1285, x / 3, {I64_MIN} / 3, 9007199254740993 / 1, 0 / -5, -0 / 5)", []),
    ("print(7 / 2, -7 / 2, 1 / 3, 2 / 3, 10 / 5)", []),
    ("print(1 << 62, 1 << 0, 0 << 100, 5 >> 64, -5 >> 64, -1 >> 1000, 5 >> 1, -5 >> 1, 1 >> 63)", []),
    ("x = 1\nn = 64\nprint(0)\nprint(x << n)", [], {"debug": ("0\n", 1), "release": ("0\n0\n", 0)}),
    ("x = 3\nn = 62\nprint(x << n)", [], ovf("", "-4611686018427387904\n")),
    ("x = -1\nn = 63\nprint(x << n)", []),
    ("n = -1\nprint(1)\nprint(4 >> n)", []),
    ("n = -1\nprint(4 << n)", []),
    ("print(2 ** 62, (-2) ** 63, 3 ** 0, 0 ** 0, (-1) ** 1001, 1 ** 100000000000)", []),
    ("print(2 ** 63)", [], ovf("", "-9223372036854775808\n")),
    ("n = -1\nprint(1)\nprint(2 ** n)", [], {"debug": ("1\n", 1), "release": ("1\n", 1)}),
    ("print(2.0 ** 0.5, 2 ** 0.5, 2.0 ** -1, (-8.0) ** 3, (-0.0) ** 3, (-0.0) ** 2, 0.0 ** 0.0, 1.0 ** 1e308)", []),
    ("inf = 1e308 * 10\nnan = inf - inf\nprint(nan ** 0, 1.0 ** nan, (-1.0) ** inf, 0.5 ** inf, 0.5 ** -inf, inf ** -1, (-inf) ** 3, (-inf) ** -3, 2.0 ** -1080)", []),
    ("print(1)\nx = 10.0 ** 400\nprint(x)", []),
    ("x = -8.0\nprint(1)\nprint(x ** 0.5)", [], {"debug": ("1\n", 1), "release": ("1\n", 1)}),
    ("x = 0.0\nprint(1)\nprint(x ** -1)", []),
    (f"print({I64_MAX} + 0, {I64_MIN} - 0, -{I64_MAX})", []),
    (f"x = {I64_MAX}\nprint(5)\nprint(x + 1)", [], ovf("5\n", "5\n-9223372036854775808\n")),
    (f"x = {I64_MIN}\nprint(x - 1)", [], ovf("", "9223372036854775807\n")),
    (f"x = {I64_MIN}\nprint(-x)", [], ovf("", "-9223372036854775808\n")),
    (f"x = {I64_MIN}\nprint(abs(x))", [], ovf("", "-9223372036854775808\n")),
    ("x = 3037000500\nprint(x * x)", [], ovf("", "-9223372036709301616\n")),
    ("x = 3037000499\nprint(x * x, -x * x)", []),
    ("print(0.1, 1.0, -0.0, 1e16, 1e15, 123456789012345678.0, 0.0001, 0.00001, 1/3, 2.5e-5, 1e308 * 10, -(1e308 * 10))", []),
    ("inf = 1e308 * 10\nprint(inf - inf, -(inf - inf), 5e-324, 1.7976931348623157e308, 1e22, 1e21, 9999999999999998.0, 0.1 + 0.2)", []),
    ("inf = 1e308 * 10\nnan = inf - inf\nprint(nan == nan, nan != nan, nan < 1, nan >= 1, 1 < nan, 1 == nan, 1 != nan)", []),
    ("inf = 1e308 * 10\nnan = inf - inf\nif nan:\n    print(1)\nprint(not nan, bool(nan), bool(-0.0), not -0.0)", []),
    ("print(2**53 + 1 == 9007199254740992.0, 2**53 + 1 > 9007199254740992.0, 9223372036854775807 < 9.223372036854776e18, 9223372036854775807 == 9.223372036854776e18)", []),
    (f"inf = 1e308 * 10\nprint({I64_MIN} == -9.223372036854776e18, {I64_MIN} < -0.0, {I64_MAX} < inf, {I64_MIN} > -inf, 0 == -0.0, 1 < 1.5 < 2 < 2.5)", []),
    ("def f(x: int) -> int:\n    print(x)\n    return x\nprint(f(1) < f(2) < f(0) < f(3))", []),
    ("def f(x: int) -> int:\n    print(x)\n    return x\nprint(f(1) < f(0) < f(3))\nprint(f(5) > f(4) > f(3) > f(2))", []),
    ("def f(x: int) -> int:\n    print(x)\n    return x\nprint(f(10), f(20), f(30))", []),
    ("def f(x: int) -> bool:\n    print(x)\n    return x > 1\nprint(f(1) and f(2), f(2) or f(3), f(2) and f(3))", []),
    ("def f(x: int) -> int:\n    print(x)\n    return x\nprint(f(1), 1 // (f(2) - 2))", []),
    ("def g():\n    print(7)\nprint(g(), g())", []),
    ("for i in range(10, 0, -3):\n    print(i)\nprint(i)", []),
    ("for i in range(0, 10, 3):\n    i = i * 100\n    print(i)\nprint(i)", []),
    ("n = 0\nfor i in range(5, 5):\n    n += 1\nfor i in range(5, 0):\n    n += 1\nfor i in range(0, 5, -1):\n    n += 1\nprint(n)", []),
    (f"for i in range({I64_MAX} - 2, {I64_MAX}):\n    print(i)\nfor i in range({I64_MIN} + 2, {I64_MIN}, -1):\n    print(i)", []),
    (f"n = 0\nfor i in range({I64_MIN}, {I64_MAX}, {I64_MAX}):\n    print(i)\n    n += 1\nprint(n)", []),
    (f"for i in range({I64_MAX}, {I64_MIN}, {I64_MIN}):\n    print(i)", []),
    ("s = 0\nprint(1)\nfor i in range(0, 5, s):\n    print(i)", []),
    ("i = 0\nwhile True:\n    i += 1\n    if i % 2 == 0:\n        continue\n    if i > 7:\n        break\n    j = 0\n    while j < 3:\n        j += 1\n        if j == 2:\n            break\n        print(i, j)\nprint(i)", []),
    ("print(1)\nprint(2)\nx = 0\nprint(1 / x)", []),
    ("print(1)\nx = 0.0\nprint(1.0 / x)", []),
    ("print(1)\nx = 0\nprint(5 % x)", []),
    ("print(1)\nx = 0.0\nprint(5.0 // x)", []),
    ("print(1)\nx = -0.0\nprint(5.0 % x)", []),
    ("print(int(2.9), int(-2.9), int(-0.5), int(1e18), int(-9.223372036854776e18), float(3), float(-0), bool(0), bool(-3), bool(0.0))", []),
    ("inf = 1e308 * 10\nprint(1)\nprint(int(inf - inf))", []),
    ("inf = 1e308 * 10\nprint(1)\nprint(int(-inf))", []),
    ("print(1)\nprint(int(9.3e18))", [], {"debug": ("1\n", 1), "release": ("1\n", 1)}),
    ("print(abs(-5), abs(5), abs(-2.5), abs(-0.0), abs(0))", []),
    ("print(not 0, not 3, not 0.0, not 2.5, True == True, True != False, True, False, None)", []),
    ("print()\nprint()\nprint(1)", []),
    ("x = print()\nprint(x)", []),
    ("from cudra import arg_int, arg_float\nprint(arg_int(1), arg_float(2), arg_float(3), arg_int(4))", ["7", "2.5", "-0.0", " -42 "]),
    ("from cudra import arg_int\nprint(1)\nprint(arg_int(2))", ["7"]),
    ("from cudra import arg_int\nprint(1)\nprint(arg_int(1))", ["x7"]),
    ("from cudra import arg_float\nprint(arg_float(1), arg_float(2), arg_float(3), arg_float(4))", ["inf", "-nan", "1e-400", "1_0.5"]),
    ("from cudra import arg_int\nprint(arg_int(0 - 0 + 1) * 2)", ["21"]),
    ("from cudra import arg_int\nn = arg_int(1)\nprint(1)\nprint(arg_int(n))", ["-1"]),
    ("import cudra\nprint(cudra.arg_int(1) + 1)", ["41"]),
    ("def fib(n: int) -> int:\n    if n < 2:\n        return n\n    return fib(n - 1) + fib(n - 2)\nprint(fib(20))", []),
    ("def ev(n: int) -> bool:\n    if n == 0:\n        return True\n    return od(n - 1)\ndef od(n: int) -> bool:\n    if n == 0:\n        return False\n    return ev(n - 1)\nprint(ev(10), od(7), ev(7))", []),
    ("def f(a: int, b: float, c: bool) -> float:\n    if c:\n        return a * b\n    return b - a\nprint(f(3, 0.5, True), f(3, 0.5, False))", []),
    ("x = 5\nx += 3\nx -= 1\nx *= 2\nx //= 3\nx %= 4\nx **= 3\nx <<= 2\nx >>= 1\nx &= 12\nx |= 3\nx ^= 5\nprint(x)", []),
    ("y = 5.0\ny += 1\ny -= 0.5\ny *= 2\ny /= 4\ny //= 0.5\ny %= 3\ny **= 2\nprint(y)", []),
    ("x = 7\nprint(-x, +x, ~x, -(-x), ~~x, -2 ** 2, (-2) ** 2, 2 ** 3 ** 2, not x == 7)", []),
    ("print(1 + 2 * 3 - 4 // 3 % 2, 7 & 3 | 8 ^ 1, 1 << 2 + 1, -7 // 2 * 2, 2 * 3.5)", []),
    ("x = 1\nif x > 5: print(1)\nelif x > 0: print(2)\nelse: print(3)\nwhile x < 3: x += 1\nprint(x)", []),
    ("def f() -> int:\n    i = 0\n    while True:\n        i += 1\n        if i == 4:\n            return i\nprint(f())", []),
    ("t = 0\nfor i in range(1000000):\n    t += i\nprint(t)", []),
    ("t = 0.0\nfor i in range(1000):\n    t += 0.1\nprint(t)", []),
]


def run(cmd, timeout=30, env=None):
    try:
        p = subprocess.run(cmd, capture_output=True, timeout=timeout, env=env)
        return p.returncode, p.stdout.decode("utf-8", "replace"), p.stderr.decode("utf-8", "replace")
    except subprocess.TimeoutExpired:
        return None, "", "TIMEOUT"


def main():
    cudrac = sys.argv[1]
    work = tempfile.mkdtemp(prefix="run_attack_")
    shim = os.path.join(work, "shim")
    os.makedirs(shim)
    open(os.path.join(shim, "cudra.py"), "w").write(SHIM)
    env = dict(os.environ, PYTHONPATH=shim)
    fails = []
    for i, case in enumerate(CASES):
        src, args = case[0], case[1]
        div = case[2] if len(case) > 2 else None
        path = os.path.join(work, f"r{i}.cdr")
        open(path, "w").write(src + "\n")
        prc, pout, perr = run(["python3", path] + args, env=env)
        if div is None and prc not in (0, 1):
            fails.append(("CASO_INVALIDO", src, f"python rc={prc} {perr[-300:]}"))
            continue
        if div is None and prc == 1 and not perr.strip().splitlines()[-1].split(":")[0].endswith("Error"):
            fails.append(("CASO_INVALIDO", src, perr[-300:]))
            continue
        if div is not None and all(div[m] == (pout, prc) for m in div):
            fails.append(("DIVERGENCIA_FALSA", src, f"python dá {pout!r} rc={prc}"))
            continue
        if cudrac == "-":
            continue
        for mode in ("debug", "release"):
            exe = os.path.join(work, f"r{i}_{mode}")
            rc, out, err = run([cudrac, "build", path, "-o", exe] + (["--release"] if mode == "release" else []), 120)
            if rc != 0:
                fails.append((f"COMPILACAO_FALHOU[{mode}]", src, err[-500:]))
                continue
            orc, oout, oerr = run([exe] + args, 30)
            want = div[mode] if div else (pout, prc)
            if (oout, orc) != want:
                fails.append((f"DIFERENTE[{mode}]", src, f"esperado out={want[0][-300:]!r} rc={want[1]}\n     nosso out={oout[-300:]!r} rc={orc} err={oerr[-200:]!r}"))
            elif div is None and prc == 1:
                pl = perr.strip().splitlines()[-1].split(":")[0]
                ol = oerr.strip().splitlines()[-1].split(":")[0] if oerr.strip() else ""
                if pl != ol:
                    fails.append((f"EXCECAO_DIFERENTE[{mode}]", src, f"python {pl} / nosso {ol}"))
    for kind, src, info in fails[:60]:
        print(f"FALHA {kind}\n{src}\n  -> {info}\n")
    cats = {}
    for f in fails:
        k = f[0].split("[")[0]
        cats[k] = cats.get(k, 0) + 1
    print(f"casos: {len(CASES)}")
    print(f"RESUMO ataque run: {len(fails)} falhas {cats}")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
