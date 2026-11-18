#!/usr/bin/env python3
"""Fit Lagom's token estimate against Claude's real tokenizer (CMP-10).

    ANTHROPIC_API_KEY=… python3 -I scripts/token_fit.py [--models claude-opus-5-5,claude-haiku-4-5]
        [--lagom target/release/lagom] [--out bench/token-factors.json]

`lagom stats` estimates tokens as bytes ÷ 4, which is wrong for JSON, code and CJK text.
This counts the real tokens of every corpus case, before and after compression, with
Anthropic's free `count_tokens` endpoint (no model runs, nothing is billed), one count
per model: newer Claude models produce about 30% more tokens for the same text. It sorts
each text into a content class (code, prose, JSON, CJK), fits bytes per token for each
class and model, and reports the error of bytes ÷ 4 and of the fitted factors, per text
and on totals. The corpus is Lagom's own generated test output (bench/corpus/,
tests/fixtures/compress/), with no personal data in it.

Standard library only, like the project's other scripts: the endpoint is called over
HTTPS directly rather than through the Anthropic SDK.
"""
import argparse
import datetime
import json
import os
import statistics
import subprocess
import sys
import tempfile
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ENDPOINT = "https://api.anthropic.com/v1/messages/count_tokens"
CLASSES = ("code", "prose", "json", "cjk")
CODE_MARKS = ("{", "}", ";", "=>", "->", "::", "def ", "fn ", "func ", "import ", "#include", "return ", "    ")


def content_class(text):
    """`cjk` when over a tenth of the characters are CJK, `json` when it parses as JSON or
    JSON Lines, `code` when a third of its lines look like code, else `prose` (which here
    includes logs and test output)."""
    chars = [c for c in text if not c.isspace()]
    if chars and sum(1 for c in chars if is_cjk(c)) / len(chars) > 0.1:
        return "cjk"
    stripped = text.strip()
    if stripped[:1] in ("{", "["):
        try:
            json.loads(stripped)
            return "json"
        except ValueError:
            lines = [l for l in stripped.splitlines() if l.strip()]
            parsed = sum(1 for l in lines if is_json(l))
            if lines and parsed / len(lines) > 0.5:
                return "json"
    lines = [l for l in text.splitlines() if l.strip()]
    if lines and sum(1 for l in lines if any(m in l for m in CODE_MARKS)) / len(lines) > 1 / 3:
        return "code"
    return "prose"


def is_cjk(c):
    o = ord(c)
    return 0x3040 <= o <= 0x30FF or 0x3400 <= o <= 0x9FFF or 0xAC00 <= o <= 0xD7AF or 0xF900 <= o <= 0xFAFF


def is_json(line):
    try:
        json.loads(line)
        return True
    except ValueError:
        return False


def count_tokens(model, text, key):
    """Claude's count for one user message holding `text`."""
    body = json.dumps({"model": model, "messages": [{"role": "user", "content": text}]}).encode()
    req = urllib.request.Request(ENDPOINT, data=body, method="POST", headers={
        "x-api-key": key, "anthropic-version": "2023-06-01", "content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.load(resp)["input_tokens"]


def samples(dirs, lagom):
    """(name, text) for every case's input and, with a lagom binary, its compressed output."""
    found = []
    home = tempfile.mkdtemp(prefix="lagom-token-fit-")
    for d in dirs:
        for case, _, files in sorted(os.walk(os.path.join(ROOT, d))):
            if "input.txt" not in files:
                continue
            with open(os.path.join(case, "input.txt"), errors="replace") as f:
                raw = f.read()
            if not raw.strip():
                continue
            name = os.path.relpath(case, ROOT)
            found.append((f"{name} (input)", raw))
            if lagom:
                out = subprocess.run([lagom, "compress"], input=raw, capture_output=True, text=True,
                                     env=dict(os.environ, LAGOM_HOME=home)).stdout
                if out.strip() and out != raw:
                    found.append((f"{name} (compressed)", out))
    return found


def fit(texts, counts, overhead):
    """Per class: bytes per token, and the relative error of bytes ÷ 4 and of the fitted
    factor, per text (median and 90th percentile of |error|) and on the class total."""
    by_class = {}
    for (name, text), tokens in zip(texts, counts):
        n = len(text.encode())
        by_class.setdefault(content_class(text), []).append((n, max(tokens - overhead, 1)))
    result = {}
    for cls, rows in sorted(by_class.items()):
        total_bytes = sum(b for b, _ in rows)
        total_tokens = sum(t for _, t in rows)
        factor = total_bytes / total_tokens

        def errors(divisor):
            per_text = sorted(abs(b / divisor - t) / t for b, t in rows)
            return {"median": statistics.median(per_text),
                    "p90": per_text[min(len(per_text) - 1, int(0.9 * len(per_text)))],
                    "total": (total_bytes / divisor - total_tokens) / total_tokens}

        result[cls] = {"texts": len(rows), "bytes": total_bytes, "tokens": total_tokens,
                       "bytes_per_token": round(factor, 3), "error_bytes_div_4": errors(4.0),
                       "error_fitted": errors(factor)}
    return result


def table(model, classes):
    lines = [f"\n{model}", f"{'class':<6} {'texts':>5} {'bytes/token':>11}   {'bytes ÷ 4: median / p90 / total':>34}   {'fitted: median / p90':>22}"]
    for cls, x in classes.items():
        a, b = x["error_bytes_div_4"], x["error_fitted"]
        lines.append(f"{cls:<6} {x['texts']:>5} {x['bytes_per_token']:>11.2f}   "
                     f"{a['median']:>9.1%} / {a['p90']:>6.1%} / {a['total']:>+7.1%}   {b['median']:>9.1%} / {b['p90']:>6.1%}")
    return "\n".join(lines)


def main(argv):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--models", default="claude-opus-5-5,claude-haiku-4-5",
                   help="one fit per model (default: a current and an older tokenizer)")
    p.add_argument("--dirs", default="bench/corpus,tests/fixtures/compress")
    p.add_argument("--lagom", default=os.path.join(ROOT, "target", "release", "lagom"),
                   help="also fit compressed outputs (skipped when the binary is missing)")
    p.add_argument("--out", default=os.path.join(ROOT, "bench", "token-factors.json"))
    args = p.parse_args(argv)
    key = os.environ.get("ANTHROPIC_API_KEY")
    if not key:
        print("token_fit: set ANTHROPIC_API_KEY (count_tokens is free, but needs a key)", file=sys.stderr)
        return 2
    texts = samples(args.dirs.split(","), args.lagom if os.path.exists(args.lagom) else None)
    report = {"date": datetime.date.today().isoformat(), "texts": len(texts), "models": {}}
    for model in args.models.split(","):
        overhead = count_tokens(model, ".", key) - 1   # the message's own framing
        counts = [count_tokens(model, text, key) for _, text in texts]
        classes = fit(texts, counts, overhead)
        report["models"][model] = {"overhead": overhead, "classes": classes}
        print(table(model, classes))
    with open(args.out, "w") as f:
        json.dump(report, f, indent=2)
        f.write("\n")
    print(f"\nwritten: {os.path.relpath(args.out, ROOT)} ({len(texts)} texts)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
