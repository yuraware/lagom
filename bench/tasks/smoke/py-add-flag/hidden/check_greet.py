import subprocess
import sys


def run(*args):
    p = subprocess.run([sys.executable, "-I", "greet.py", *args], capture_output=True, text=True)
    assert p.returncode == 0, p.stderr
    return p.stdout


assert run("Ann") == "Hello, Ann!\n", run("Ann")
assert run("--shout", "Ann") == "HELLO, ANN!\n", run("--shout", "Ann")
assert run("Ann", "--shout") == "HELLO, ANN!\n"
print("ok")
