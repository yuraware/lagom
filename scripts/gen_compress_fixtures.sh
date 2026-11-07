#!/bin/bash
# Generates compressor fixtures from commands run on scratch projects we create here.
set -u
S="$1"; OUT="$2"; W="$S/work"
rm -rf "$W" "$OUT"; mkdir -p "$W" "$OUT"
case_() { # name exit_code must_keep_json < input
  mkdir -p "$OUT/$1"
  sed -e "s#$W#/work#g" -e "s#$HOME#/home/dev#g" > "$OUT/$1/input.txt"
  printf '{"exit_code": %s, "must_keep": %s}\n' "$2" "$3" > "$OUT/$1/meta.json"
}
cap() { "$@" 2>&1; echo "__EXIT__$?"; }
run() { # name must_keep cmd...   (records exit code)
  local name="$1" keep="$2"; shift 2
  local out code; out=$("$@" 2>&1); code=$?
  printf '%s\n' "$out" | case_ "$name" "$code" "$keep"
}

# --- Rust crate with passing and failing tests, and a build error -------------
cd "$W" && cargo new -q crate_ok && cd crate_ok
python3 - <<'PY'
body = "pub fn add(a: i32, b: i32) -> i32 { a + b }\n#[cfg(test)]\nmod tests {\n    use super::*;\n"
for i in range(60):
    body += f"    #[test]\n    fn adds_{i}() {{ assert_eq!(add({i}, 1), {i + 1}); }}\n"
body += "}\n"
open("src/lib.rs", "w").write(body)
PY
cargo build -q 2>/dev/null
run cargo-test-pass '["test result: ok. 60 passed"]' cargo test --color never
python3 - <<'PY'
t = open("src/lib.rs").read()
t = t.replace("assert_eq!(add(17, 1), 18)", "assert_eq!(add(17, 1), 99)")
t = t.replace("assert_eq!(add(42, 1), 43)", "assert_eq!(add(42, 1), 0, \"bad sum\")")
open("src/lib.rs", "w").write(t)
PY
run cargo-test-fail '["tests::adds_17", "tests::adds_42", "panicked at src/lib.rs", "bad sum", "test result: FAILED"]' cargo test --color never
cat > src/main.rs <<'RS'
fn main() {
    let unused = 5;
    let n: u32 = "seven";
    println!("{}", n);
}
RS
run cargo-build-error '["error[E0308]: mismatched types", "--> src/main.rs:3:18", "could not compile"]' cargo build --color never
run cargo-build-error-color '["E0308", "mismatched types", "src/main.rs:3:18"]' cargo build --color always

# --- git with many commits and a dirty tree ------------------------------------
cd "$W" && mkdir repo && cd repo && git init -q && git config user.email dev@example.com && git config user.name Dev
for i in $(seq 1 120); do
  f="src/mod_$((i % 25)).rs"; mkdir -p src
  echo "pub fn f$i() -> u32 { $i }" >> "$f"
  GIT_AUTHOR_DATE="2026-01-01T00:00:$((i % 60))" GIT_COMMITTER_DATE="2026-01-01T00:00:$((i % 60))" git add -A >/dev/null && \
  GIT_AUTHOR_DATE="2026-01-01T00:00:$((i % 60))" GIT_COMMITTER_DATE="2026-01-01T00:00:$((i % 60))" git commit -q -m "add f$i to mod_$((i % 25))"
done
run git-log-stat '["add f120 to mod_20", "add f1 to mod_1"]' git log --stat
for i in $(seq 1 60); do echo "new" > "untracked_$i.txt"; done
for i in 1 2 3; do echo "// edit" >> "src/mod_$i.rs"; done
run git-status-long '["modified:   src/mod_1.rs", "Untracked files:"]' git status

# --- generated source tree for rg / find / ls ----------------------------------
cd "$W" && python3 - <<'PY'
import os
for d in range(12):
    os.makedirs(f"tree/pkg_{d}/sub", exist_ok=True)
    for f in range(15):
        with open(f"tree/pkg_{d}/mod_{f}.py", "w") as fh:
            for k in range(8):
                fh.write(f"def handler_{d}_{f}_{k}(request):\n    return {k}\n\n")
        open(f"tree/pkg_{d}/sub/data_{f}.json", "w").write("{}\n")
PY
run rg-many-matches '["tree/pkg_0/mod_0.py:1:def handler_0_0_0"]' rg -n --sort path "def handler" tree
# find/ls order is not sorted: must_keep is set to the first and last lines after generation.
run find-many-files '[]' find tree -type f
cd tree && run ls-recursive '[]' ls -R . && cd ..

# --- pytest: pass, fail, collection error --------------------------------------
mkdir -p "$W/py" && cd "$W/py"
python3 - <<'PY'
for i in range(80):
    open(f"test_ok_{i}.py", "w").write(f"def test_value_{i}():\n    assert {i} + 1 == {i + 1}\n")
PY
run pytest-pass '["80 passed"]' "$S/venv/bin/python" -m pytest -p no:cacheprovider -q
cat > test_bad.py <<'PY'
def helper(x):
    return {"a": 1}[x]

def test_lookup():
    assert helper("b") == 1

def test_math():
    assert 2 + 2 == 5, "arithmetic is broken"
PY
run pytest-fail '["FAILED test_bad.py::test_lookup", "FAILED test_bad.py::test_math", "KeyError", "arithmetic is broken", "2 failed, 80 passed"]' "$S/venv/bin/python" -m pytest -p no:cacheprovider
rm test_bad.py; echo "import not_a_real_module_xyz" > test_broken.py
run pytest-collection-error '["ERROR collecting test_broken.py", "ModuleNotFoundError", "not_a_real_module_xyz", "Interrupted: 1 error during collection"]' "$S/venv/bin/python" -m pytest -p no:cacheprovider
rm test_broken.py

# --- Python traceback with chained exceptions ----------------------------------
cat > "$W/chain.py" <<'PY'
import json
def load(raw):
    try:
        return json.loads(raw)
    except ValueError as e:
        raise RuntimeError("config is not valid JSON") from e
for i in range(150):
    print(f"processing record {i}")
load("{bad")
PY
cd "$W" && run python-traceback '["Traceback (most recent call last):", "json.decoder.JSONDecodeError", "The above exception was the direct cause", "RuntimeError: config is not valid JSON"]' python3 chain.py

# --- make with a failing step among many --------------------------------------
cat > "$W/Makefile" <<'MK'
STEPS := $(shell seq 1 90)
all: $(STEPS:%=step%) broken
step%:
	@echo "building target $* ... ok"
broken:
	@echo "linking app"; echo "ld: undefined symbol: _start_engine" >&2; exit 2
MK
run make-error '["undefined symbol: _start_engine", "make: *** [broken] Error 2"]' make -C "$W" --no-print-directory

# --- synthetic: progress bar, repeats, long lines, empty -----------------------
python3 -c '
import sys
for i in range(0, 101, 5):
    sys.stdout.write(f"\rDownloading model.bin [{"#" * (i // 5):<20}] {i}%")
sys.stdout.write("\nDownload complete: 104857600 bytes\n")
for i in range(200):
    print("npm WARN deprecated inflight@1.0.6: This module is not supported")
print("npm ERR! code ERESOLVE")
print("npm ERR! unable to resolve dependency tree")
' | case_ progress-and-repeats 1 '["Download complete: 104857600 bytes", "npm ERR! code ERESOLVE", "unable to resolve dependency tree"]'
python3 -c '
import json
print(json.dumps({"items": [{"id": i, "name": f"item-{i}", "tags": ["a", "b"]} for i in range(400)]}))
print("error: response truncated at item 400")
' | case_ long-json-line 0 '["error: response truncated at item 400"]'
printf '' | case_ empty-output 0 '[]'
printf 'Compiling serde v1.0.%s\n' $(seq 100 180) | case_ numbered-run 0 '["Compiling serde v1.0.100", "Compiling serde v1.0.180"]'
echo "done"
