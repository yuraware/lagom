import threading

hits = 0


def record_hit():
    global hits
    hits += 1


def serve(n_threads=8, n_requests=10_000):
    def worker():
        for _ in range(n_requests):
            record_hit()

    threads = [threading.Thread(target=worker) for _ in range(n_threads)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    return hits
