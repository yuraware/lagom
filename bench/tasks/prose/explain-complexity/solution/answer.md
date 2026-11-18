It's O(n²): for every item it scans all the items after it, so the number of comparisons grows with the square of the list's length (the `a not in found` check adds a little more when there are many duplicates).

To make it linear, walk the list once and keep a set of the values seen so far: when a value is already in the set, it's a duplicate. Set lookups are O(1) on average, so the whole function becomes O(n).
