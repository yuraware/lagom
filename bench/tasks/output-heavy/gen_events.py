"""The JSON task (BEN-09): `write N` writes data/events.json (N events, pretty-printed);
`solve N` writes failures.json; `check N` compares failures.json with the counts from the
same N events, generated again rather than read back, so editing the data doesn't help."""
import json
import os
import sys

SERVICES = ["auth", "billing", "catalog", "checkout", "email", "images", "inventory", "notify",
            "orders", "payments", "search", "shipping"]


def events(n):
    x = 7
    for i in range(n):
        x = (x * 6364136223846793005 + 1442695040888963407) % 2**64
        service = SERVICES[x % len(SERVICES)]
        r = (x >> 16) % 100
        status = "failed" if r < 4 and service not in ("email", "images") else "timeout" if r < 7 else "ok"
        yield {"id": f"evt-{i:05d}", "service": service, "status": status, "ms": (x >> 32) % 3000}


def failures(n):
    counts = {}
    for e in events(n):
        if e["status"] == "failed":
            counts[e["service"]] = counts.get(e["service"], 0) + 1
    return counts


if __name__ == "__main__":
    mode, n = sys.argv[1], int(sys.argv[2])
    if mode == "write":
        os.makedirs("data", exist_ok=True)
        with open("data/events.json", "w") as f:
            json.dump(list(events(n)), f, indent=2)
    elif mode == "solve":
        with open("failures.json", "w") as f:
            json.dump(failures(n), f)
    else:
        try:
            got = json.load(open("failures.json"))
        except (OSError, ValueError) as e:
            sys.exit(f"failures.json: {e}")
        want = failures(n)
        sys.exit(0 if got == want else f"failures.json is {got}, expected {want}")
