#!/bin/bash
# Generates the benchmark corpus (bench/corpus/<tool>/<case>/{input.txt,meta.json}) from
# commands run here on scratch projects. Never uses ../samples, the network or personal data.
#   scripts/gen_bench_corpus.sh            # writes bench/corpus
# Complements tests/fixtures/compress (golden cases); `lagom bench compress` reads both.
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT="$ROOT/bench/corpus"
W=$(mktemp -d "${TMPDIR:-/tmp}/lagom-corpus.XXXXXX"); trap 'rm -rf "$W"' EXIT
W=$(cd "$W" && pwd -P)   # real path: tools print it resolved (/private/var/... on macOS)
WSHORT=${W#/private}
rm -rf "$OUT"; mkdir -p "$OUT"
export NO_COLOR=1 CARGO_TERM_COLOR=never GIT_PAGER=cat PAGER=cat

save() { # tool case exit must_keep_json  < output
  local d="$OUT/$1/$2"; mkdir -p "$d"
  head -c 400000 | sed -e "s#$W#/work#g" -e "s#$WSHORT#/work#g" -e "s#$HOME#/home/dev#g" > "$d/input.txt"
  printf '{"exit_code": %s, "must_keep": %s}\n' "$3" "$4" > "$d/meta.json"
}
run() { # tool case must_keep_json cmd...
  local tool=$1 name=$2 keep=$3; shift 3
  local out code; out=$("$@" 2>&1 </dev/null); code=$?  # no command may wait for input
  printf '%s\n' "$out" | save "$tool" "$name" "$code" "$keep"
}
have() { command -v "$1" >/dev/null; }
gap() { # tool case task: a must_keep Lagom doesn't meet yet; the task that fixes it
  python3 -I -c 'import json,sys; p=sys.argv[1]; m=json.load(open(p)); m["known_gap"]=sys.argv[2]; open(p,"w").write(json.dumps(m)+"\n")' "$OUT/$1/$2/meta.json" "$3"
}

# ---------- Rust ----------
cd "$W" && cargo new -q rs && cd rs
cat > src/lib.rs <<'RS'
pub fn parse(s: &str) -> i32 { s.parse().unwrap() }
pub fn unused_helper(x: i32) -> i32 { let y = x; x }
#[cfg(test)]
mod tests {
    use super::*;
RS
for i in $(seq 1 120); do echo "    #[test] fn t$i() { assert_eq!(parse(\"$i\"), $i); }" >> src/lib.rs; done
echo '    #[test] fn bad_parse() { parse("x"); }' >> src/lib.rs
echo '    #[test] fn wrong_sum() { assert_eq!(parse("2") + 2, 5, "math is off"); }' >> src/lib.rs
echo '}' >> src/lib.rs
cargo build -q 2>/dev/null
run rust cargo-test-two-failures '["tests::bad_parse", "tests::wrong_sum", "math is off", "test result: FAILED"]' cargo test
run rust cargo-test-verbose-names '["test result: FAILED"]' cargo test -- --test-threads 1
run rust cargo-clippy-warnings '["unused variable", "src/lib.rs:2"]' cargo clippy --all-targets
cat > src/main.rs <<'RS'
use std::collections::HashMap;
fn main() {
    let m: HashMap<String, u32> = HashMap::new();
    let v: Vec<u32> = m.values().collect();
    let s: String = 5;
    undefined_fn();
}
RS
run rust cargo-build-three-errors '["error[E0277]", "error[E0308]", "error[E0425]", "src/main.rs:5", "could not compile"]' cargo build
printf 'fn main() { let x: i32 = "a"; }\n' > "$W/one.rs"
run rust rustc-single-error '["error[E0308]", "one.rs:1"]' rustc --edition 2021 "$W/one.rs" -o "$W/one"
rm src/main.rs; sed -i '' 's/let y = x; x/x/' src/lib.rs; sed -i '' -E '/bad_parse|wrong_sum/d' src/lib.rs
run rust cargo-test-all-pass '["test result: ok. 120 passed"]' cargo test
run rust cargo-doc '["Finished"]' cargo doc --no-deps

# ---------- Python ----------
mkdir -p "$W/py" && cd "$W/py"
python3 - <<'PY'
for i in range(150):
    open(f"test_ok_{i:03}.py", "w").write(f"import unittest\nclass T(unittest.TestCase):\n    def test_{i}(self):\n        self.assertEqual({i} * 2, {i * 2})\n")
open("test_fail.py", "w").write("""import unittest
class Broken(unittest.TestCase):
    def test_dict(self):
        self.assertEqual({"a": 1, "b": [1, 2]}, {"a": 1, "b": [1, 3]})
    def test_raise(self):
        raise ValueError("bad config value: retries=-1")
""")
PY
run python unittest-fail '["FAIL: test_dict", "ERROR: test_raise", "ValueError: bad config value: retries=-1", "FAILED (failures=1, errors=1)"]' python3 -m unittest discover -v
rm test_fail.py
run python unittest-pass '["OK"]' python3 -m unittest discover
cat > "$W/deep.py" <<'PY'
def a(n):
    return b(n - 1)
def b(n):
    return c({"k": n})
def c(d):
    return d["missing"]
for i in range(300):
    print(f"[worker-{i % 4}] processed batch {i} in {i % 17 + 3}ms")
a(3)
PY
run python traceback-deep '["Traceback (most recent call last):", "deep.py\", line 6", "KeyError: '"'"'missing'"'"'"]' python3 "$W/deep.py"
run python json-tool-error '["Expecting"]' sh -c 'echo "{\"a\": 1,, }" | python3 -m json.tool'
run python import-error '["ModuleNotFoundError: No module named '"'"'notinstalled_pkg_xyz'"'"'"]' python3 -c 'import notinstalled_pkg_xyz'
python3 -m venv "$W/venv" >/dev/null 2>&1 && \
  run python pip-no-index-missing '["notarealpkg-zzz"]' "$W/venv/bin/python" -m pip install --no-index notarealpkg-zzz

# ---------- Go ----------
if have go; then
  mkdir -p "$W/gomod" && cd "$W/gomod" && go mod init example.com/m >/dev/null 2>&1
  cat > m.go <<'GO'
package m
func Add(a, b int) int { return a + b }
GO
  { echo 'package m'; echo 'import "testing"'; for i in $(seq 1 60); do echo "func TestAdd$i(t *testing.T) { if Add($i, 1) != $((i + 1)) { t.Fatal(\"bad\") } }"; done
    echo 'func TestBroken(t *testing.T) { if Add(2, 2) != 5 { t.Errorf("Add(2,2) = %d, want 5", Add(2, 2)) } }'; } > m_test.go
  run go go-test-fail '["--- FAIL: TestBroken", "Add(2,2) = 4, want 5", "FAIL"]' go test -v ./...
  sed -i '' '/TestBroken/d' m_test.go
  run go go-test-pass '["ok"]' go test -v ./...
  printf 'package m\nfunc Bad() int { return "x" }\n' > bad.go
  run go go-build-error '["bad.go:2", "cannot use"]' go build ./...
  rm bad.go
  printf 'package m\nimport "fmt"\nfunc V() { fmt.Printf("%%d\\n", "s") }\n' > vet.go
  run go go-vet '["vet.go:3", "wrong type"]' go vet ./...
fi

# ---------- Node / npm (offline) ----------
if have node; then
  mkdir -p "$W/js" && cd "$W/js"
  cat > app.js <<'JS'
for (let i = 0; i < 250; i++) console.log(`GET /api/items/${i} 200 ${i % 9 + 1}ms`);
function load(cfg) { return cfg.database.url; }
load({});
JS
  run node node-stack-trace '["TypeError: Cannot read properties of undefined", "app.js:2"]' node app.js
  { echo "const test = require('node:test'); const assert = require('node:assert');"
    for i in $(seq 1 60); do echo "test('adds $i', () => assert.strictEqual($i + 1, $((i + 1))));"; done
    echo "test('parses config', () => assert.deepStrictEqual({retries: 3}, {retries: -1}));"; } > app.test.js
  run node node-test-fail '["✖ parses config", "fail 1", "ERR_ASSERTION"]' node --test app.test.js
fi

# ---------- C / Swift compilers ----------
cat > "$W/bad.c" <<'C'
#include <stdio.h>
int main(void) {
    int x = "text";
    undeclared_call();
    printf("%d\n", y);
    return 0
}
C
run c clang-errors '["bad.c:4", "bad.c:5", "error:"]' cc -Wall -c "$W/bad.c" -o "$W/bad.o"
if have swiftc; then
  printf 'let x: Int = "s"\nprint(y)\n' > "$W/bad.swift"
  run swift swiftc-errors '["bad.swift:1", "error:"]' swiftc -typecheck "$W/bad.swift"
fi

# ---------- git ----------
cd "$W" && mkdir g && cd g && git init -q && git config user.email dev@example.com && git config user.name Dev
for i in $(seq 1 200); do
  mkdir -p "pkg$((i % 10))"; printf 'value = %s\nname = "item-%s"\n' "$i" "$i" > "pkg$((i % 10))/f$i.toml"
done
printf '[package]\nname = "demo"\nversion = "0.1.0"\n' > Cargo.toml
for i in $(seq 1 300); do printf '[[package]]\nname = "dep-%s"\nversion = "1.%s.0"\n' "$i" "$i" >> Cargo.lock; done
git add -A && GIT_AUTHOR_DATE=2026-01-01T00:00:00 GIT_COMMITTER_DATE=2026-01-01T00:00:00 git commit -q -m "initial import"
for i in $(seq 1 80); do
  echo "changed = $i" >> "pkg$((i % 10))/f$i.toml"
  GIT_AUTHOR_DATE="2026-01-02T00:$((i % 60)):00" GIT_COMMITTER_DATE="2026-01-02T00:$((i % 60)):00" git commit -qam "tune item $i"
done
for i in $(seq 1 120); do sed -i '' "s/item-$i\"/renamed-$i\"/" "pkg$((i % 10))/f$i.toml"; done
for i in $(seq 1 300); do sed -i '' "s/version = \"1.$i.0\"/version = \"2.$i.0\"/" Cargo.lock; done
run git git-diff-with-lockfile '["renamed-1\"", "renamed-120\"", "Cargo.lock"]' git diff
run git git-diff-stat '["files changed"]' git diff --stat
run git git-log-oneline '["tune item 80", "initial import"]' git log --oneline
run git git-log-patch '["tune item 80", "changed = 80"]' git log -p -5
for b in $(seq 1 60); do git branch "feature/item-$b"; done
run git git-branch-many '["feature/item-1", "main"]' git branch -a
run git git-status-porcelain '["pkg1/f1.toml"]' git status --short
gap git git-status-porcelain FLT-07
run git git-grep '["pkg0/f10.toml", "f99.toml (1)"]' git grep -n "name ="  # later files are listed per folder
run git git-show-stat '["tune item 80"]' git show --stat HEAD
run git git-not-a-repo '["fatal: not a git repository"]' git -C / status
run git git-bad-ref '["fatal:"]' git log no-such-branch

# ---------- search / files ----------
cd "$W" && python3 - <<'PY'
import os
for d in range(8):
    os.makedirs(f"src/mod{d}", exist_ok=True)
    for f in range(20):
        with open(f"src/mod{d}/file{f}.rs", "w") as fh:
            for k in range(12):
                fh.write(f"// TODO(item {d}-{f}-{k}): tidy\nfn f_{d}_{f}_{k}() {{}}\n")
PY
run search rg-context '["src/mod0/file0.rs"]' rg -n -C 1 --sort path "TODO" src
run search rg-count '["src/mod7/file9.rs:12"]' rg -c --sort path "TODO" src
run search grep-rn '["TODO(item"]' grep -rn "TODO" src
run search find-name '["src/mod0/file0.rs", "src/mod7/file9.rs"]' sh -c 'find src -name "*.rs" | sort'
run search ls-la-recursive '["file0.rs"]' ls -laR src
run search find-missing-dir '["No such file or directory"]' find /nonexistent-lagom-dir -name x
run search du-all '["src"]' du -a src

# ---------- JSON ----------
python3 - "$W/api.json" <<'PY'
import json, sys
items = [{"id": i, "name": f"service-{i}", "status": "ok" if i % 37 else "failed",
          "error": None if i % 37 else f"timeout after {i}ms", "tags": ["a", "b"]} for i in range(400)]
json.dump({"items": items, "total": 400}, open(sys.argv[1], "w"))
PY
run json jq-pretty-array '["\"status\":\"failed\"", "timeout after 37ms", "\"total\":400"]' jq . "$W/api.json"  # Lagom prints JSON compactly
run json jq-compact-lines '["timeout after 74ms"]' jq -c '.items[]' "$W/api.json"
run json jq-parse-error '["parse error"]' sh -c 'echo "{\"a\": [1, 2,}" | jq .'

# ---------- logs ----------
python3 - "$W/app.log" <<'PY'
import sys
with open(sys.argv[1], "w") as f:
    for i in range(3000):
        f.write(f"2026-11-08T10:{i // 60 % 60:02}:{i % 60:02}.{i % 1000:03}Z INFO  request id=req-{i:05x} path=/api/v1/items/{i % 50} status=200 took={i % 40 + 2}ms\n")
        if i in (1200, 2500):
            f.write(f"2026-11-08T10:{i // 60 % 60:02}:{i % 60:02}.000Z ERROR database connection lost: host=db-1 port=5432 retry=3\n")
        if i % 500 == 0:
            f.write(f"2026-11-08T10:{i // 60 % 60:02}:00.000Z WARN  slow query took 1532ms table=orders\n")
PY
run logs app-log-errors '["ERROR database connection lost", "WARN  slow query"]' cat "$W/app.log"
python3 - "$W/access.log" <<'PY'
import sys
with open(sys.argv[1], "w") as f:
    for i in range(2500):
        code = 500 if i in (700, 1800) else 200
        f.write(f'10.0.{i % 5}.{i % 250} - - [08/Nov/2026:10:{i // 60 % 60:02}:{i % 60:02} +0000] "GET /static/app.{i % 3}.js HTTP/1.1" {code} {1000 + i % 300} "-" "Mozilla/5.0"\n')
PY
run logs access-log '["\" 500 "]' cat "$W/access.log"

# ---------- containers / clusters (no daemon or cluster needed: real error output) ----------
if have kubectl; then
  run k8s kubectl-no-cluster '["127.0.0.1:1"]' kubectl --kubeconfig /dev/null --server https://127.0.0.1:1 --token dummy --insecure-skip-tls-verify --request-timeout=3s get pods
fi
if have docker; then
  run docker docker-bad-flag '["unknown flag"]' docker ps --no-such-flag
fi

# ---------- build tools ----------
cat > "$W/Makefile" <<'MK'
SRCS := $(shell seq 1 60)
all: $(SRCS:%=obj%) link
obj%:
	@echo "cc -c src/unit$*.c -o build/unit$*.o"
link:
	@echo "ld build/*.o -o app"; echo "ld: symbol(s) not found for architecture arm64: _engine_start" >&2; exit 1
MK
run make make-link-error '["symbol(s) not found", "_engine_start"]' make -C "$W" --no-print-directory

# ---------- check must_keep and secrets ----------
python3 -I - "$OUT" <<'PY'
import json, os, re, sys
root = sys.argv[1]; bad = 0; n = 0; failures = 0
for tool in sorted(os.listdir(root)):
    for case in sorted(os.listdir(f"{root}/{tool}")):
        d = f"{root}/{tool}/{case}"; n += 1
        text = open(f"{d}/input.txt", errors="replace").read()
        m = json.load(open(f"{d}/meta.json")); failures += m["exit_code"] != 0
        missing = [k for k in m["must_keep"] if k not in text]
        if missing: print(f"{tool}/{case}: must_keep not in input: {missing}"); bad += 1
        if re.search(r"(ghp_|github_pat_|sk-ant-|sk-proj-|AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY)", text):
            print(f"{tool}/{case}: looks like a secret"); bad += 1
print(f"{n} cases, {failures} failures ({failures * 100 // max(n, 1)}%), {bad} problems")
sys.exit(1 if bad else 0)
PY
