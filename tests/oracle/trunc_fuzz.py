import os, subprocess, sys, tempfile
from concurrent.futures import ThreadPoolExecutor
cudrac, root = sys.argv[1], sys.argv[2]
work = tempfile.mkdtemp(prefix="trunc_")
files = []
for d in ("tests/ui", "tests/check-ok"):
    for f in sorted(os.listdir(os.path.join(root, d))):
        if f.endswith(".cdr"):
            files.append(os.path.join(root, d, f))
jobs = []
for fi, p in enumerate(files):
    b = open(p, "rb").read()
    cuts = [i for i in range(len(b) + 1) if i == len(b) or (b[i] & 0xC0) != 0x80]
    for i in cuts:
        jobs.append((fi, i, b[:i]))
def one(job):
    fi, i, data = job
    path = os.path.join(work, f"t{fi}_{i}.cdr")
    open(path, "wb").write(data)
    out = []
    for cmd in ([cudrac, "check", path], [cudrac, "build", path, "--emit=tir"]):
        try:
            p = subprocess.run(cmd, capture_output=True, timeout=20)
            err = p.stderr.decode("utf-8", "replace")
            if p.returncode not in (0, 1) or "panicked" in err or "erro interno" in err:
                out.append(f"{files[fi]}@{i} {cmd[1]} rc={p.returncode} {err[-300:]}")
        except subprocess.TimeoutExpired:
            out.append(f"{files[fi]}@{i} TIMEOUT")
    os.remove(path)
    return out
fails = []
with ThreadPoolExecutor(8) as ex:
    for r in ex.map(one, jobs):
        fails.extend(r)
for f in fails[:20]:
    print("FALHA", f)
print(f"RESUMO truncagem: {len(jobs)} cortes, {len(fails)} falhas")
