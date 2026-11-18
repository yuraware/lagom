import os
import tempfile

from sizes import large_files


def make_tree():
    root = tempfile.mkdtemp()
    for rel, size in [("a.bin", 10), ("small.txt", 1), ("sub/b.bin", 20), ("sub/deeper/c.bin", 30),
                      ("sub/deeper/tiny", 2)]:
        path = os.path.join(root, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as f:
            f.write(b"x" * size)
    return root


def test_finds_files_in_every_subfolder():
    assert large_files(make_tree(), 10) == ["a.bin", "sub/b.bin", "sub/deeper/c.bin"]


def test_threshold_is_inclusive_and_folders_are_not_files():
    root = make_tree()
    assert large_files(root, 30) == ["sub/deeper/c.bin"]
    assert large_files(root, 1000) == []
