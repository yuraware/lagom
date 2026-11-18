from itertools import islice


def chunks(items, n):
    """Lists of `n` items from `items`, in order; the last may be shorter."""
    if n < 1:
        raise ValueError("n must be at least 1")
    it = iter(items)
    while batch := list(islice(it, n)):
        yield batch
