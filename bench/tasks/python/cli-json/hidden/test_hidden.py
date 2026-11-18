import json
import os
import subprocess
import sys
import tempfile


def run(*args):
    # Not -I: `-m` then finds the package in the current folder.
    p = subprocess.run([sys.executable, "-B", "-m", "todos", *args], capture_output=True, text=True,
                       cwd=os.getcwd())
    assert p.returncode == 0, p.stderr
    return p.stdout


def todo_file():
    f = tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False)
    f.write("buy milk\n[x] call Ann\nfix the bike\n")
    f.close()
    return f.name


def test_plain_list_is_unchanged():
    assert run("list", todo_file()) == "buy milk\nfix the bike\n"


def test_json_list():
    assert json.loads(run("list", todo_file(), "--json")) == ["buy milk", "fix the bike"]
