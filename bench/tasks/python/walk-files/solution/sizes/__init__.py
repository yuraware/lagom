from pathlib import Path


def large_files(root, min_bytes):
    """Files under `root` of at least `min_bytes`, as sorted relative paths."""
    base = Path(root)
    return sorted(
        p.relative_to(base).as_posix()
        for p in base.rglob("*")
        if p.is_file() and p.stat().st_size >= min_bytes
    )
