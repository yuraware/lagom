def open_items(text):
    """Items not marked done ("[x] ...") in a to-do file with one item per line."""
    return [line.strip() for line in text.splitlines() if line.strip() and not line.startswith("[x]")]
