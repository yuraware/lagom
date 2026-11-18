"""The log tasks (BEN-09): `write N` writes logs/app.log, N lines with one failed payment
among ordinary traffic, successful charges and an unrelated error; `check` passes when
blocklist.txt holds exactly the failed order's id."""
import os
import sys

FAILED = "ORD-7731-QX"


def lines(n):
    x = 42
    for i in range(n):
        x = (x * 6364136223846793005 + 1442695040888963407) % 2**64
        t = f"2026-11-17T02:{(i // 60) % 60:02d}:{i % 60:02d}.{x % 1000:03d}Z"
        if i == int(n * 0.37):
            yield f"{t} ERROR cache: connection lost; retrying (attempt 1/3)"
        elif i == int(n * 0.37) + 1:
            yield f"{t} INFO cache: reconnected"
        elif i == int(n * 0.71):
            yield f"{t} ERROR billing: payment declined for order {FAILED}: card expired"
        elif i == n - 1:
            yield f"{t} ERROR billing: nightly job finished with 1 failed payment"
        else:
            kind = x % 10
            if kind < 6:
                yield f"{t} INFO http: GET /api/items/{x % 9000 + 1000} 200 {x % 90 + 3}ms"
            elif kind < 8:
                yield f"{t} INFO billing: charged order ORD-{x % 9000 + 1000}-{chr(65 + x % 26)}{chr(65 + (x >> 8) % 26)}"
            elif kind == 8:
                yield f"{t} DEBUG cache: hit key=item:{x % 9000 + 1000}"
            else:
                yield f"{t} WARN http: slow request /api/search took {x % 2000 + 1000}ms"


if __name__ == "__main__":
    if sys.argv[1] == "write":
        os.makedirs("logs", exist_ok=True)
        with open("logs/app.log", "w") as f:
            f.write("\n".join(lines(int(sys.argv[2]))) + "\n")
    else:
        try:
            got = open("blocklist.txt").read().strip()
        except OSError:
            sys.exit("blocklist.txt is missing")
        sys.exit(0 if got == FAILED else f"blocklist.txt holds {got!r}, not the failed order")
