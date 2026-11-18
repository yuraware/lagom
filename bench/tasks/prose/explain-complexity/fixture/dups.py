def find_duplicates(items):
    """Values that occur more than once, in order of their second occurrence."""
    found = []
    for i, a in enumerate(items):
        for b in items[i + 1:]:
            if a == b and a not in found:
                found.append(a)
    return found
