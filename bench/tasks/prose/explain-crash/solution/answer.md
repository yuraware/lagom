`average([])` crashes with `ZeroDivisionError`: for an empty list `len(values)` is 0, so the function divides `sum(values)` (also 0) by zero.

Handle the empty case first, for example:

```python
def average(values):
    if not values:
        return 0.0  # or raise ValueError("average of an empty list")
    return sum(values) / len(values)
```
