def chunks(items, n):
    """Lists of `n` items from `items`, in order."""
    for start in range(0, len(items) - n + 1, n):
        yield items[start:start + n]
