from pathlib import Path


def large_files(root, min_bytes):
    """Files under `root` of at least `min_bytes`, as sorted relative paths."""
    return sorted(p.name for p in Path(root).iterdir() if p.is_file() and p.stat().st_size >= min_bytes)
