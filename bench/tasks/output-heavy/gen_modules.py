"""The library-move task (BEN-09): `write N` writes app/ with N modules that use oldlib's
fetch (two thirds `from oldlib import fetch`, the rest `import oldlib`); `solve N` moves them
to newlib and deletes oldlib.py; `check N` passes when oldlib.py is gone, app/ never
mentions oldlib and every module's run() returns newlib's result."""
import importlib
import os
import sys


def module(i, lib):
    if i % 3 == 2:
        return f'import {lib}\n\n\ndef run():\n    return {lib}.fetch("item-{i}")\n'
    return f'from {lib} import fetch\n\n\ndef run():\n    return fetch("item-{i}")\n'


def write(n, lib):
    os.makedirs("app", exist_ok=True)
    open("app/__init__.py", "w").close()
    for i in range(n):
        with open(f"app/mod_{i:03d}.py", "w") as f:
            f.write(module(i, lib))


def check(n):
    problems = []
    if os.path.exists("oldlib.py"):
        problems.append("oldlib.py still exists")
    for name in sorted(os.listdir("app")):
        if "oldlib" in open(os.path.join("app", name)).read():
            problems.append(f"app/{name} still mentions oldlib")
    sys.path.insert(0, os.getcwd())
    for i in range(n):
        try:
            got = importlib.import_module(f"app.mod_{i:03d}").run()
        except Exception as e:
            problems.append(f"app/mod_{i:03d}.py: {e!r}")
            continue
        if got != f"new:item-{i}":
            problems.append(f"app/mod_{i:03d}.py: run() returned {got!r}")
    print("\n".join(problems[:10]) or f"{n} modules use newlib")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.dont_write_bytecode = True
    mode, n = sys.argv[1], int(sys.argv[2])
    if mode == "write":
        write(n, "oldlib")
    elif mode == "solve":
        write(n, "newlib")
        os.remove("oldlib.py")
    else:
        sys.exit(check(n))
