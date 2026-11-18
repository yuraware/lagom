"""Runs the `test_*` functions of test_hidden.py in the current folder, the way pytest
would for plain-assert tests, so the Python suite's checks need nothing installed.
Exit 0 when all pass. The test files are valid pytest files too: `pytest test_hidden.py`.
"""
import os
import sys
import traceback

sys.path.insert(0, os.getcwd())  # -I leaves the current folder out
sys.dont_write_bytecode = True

import test_hidden  # noqa: E402

tests = [(name, f) for name, f in vars(test_hidden).items() if name.startswith("test_") and callable(f)]
failed = 0
for name, test in tests:
    try:
        test()
    except Exception:
        failed += 1
        print(f"FAILED {name}")
        traceback.print_exc(limit=-2)
print(f"{len(tests) - failed} passed, {failed} failed")
sys.exit(1 if failed or not tests else 0)
