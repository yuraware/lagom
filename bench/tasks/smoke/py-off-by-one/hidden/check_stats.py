import os
import sys

sys.path.insert(0, os.getcwd())  # -I leaves the script's folder out

from stats import moving_average  # noqa: E402

assert moving_average([1, 2, 3, 4], 2) == [1.5, 2.5, 3.5], moving_average([1, 2, 3, 4], 2)
assert moving_average([5], 1) == [5.0]
assert moving_average([1, 2], 3) == []
try:
    moving_average([1], 0)
except ValueError:
    pass
else:
    raise AssertionError("window 0 must raise ValueError")
print("ok")
