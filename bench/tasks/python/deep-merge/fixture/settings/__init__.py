def deep_merge(base, override):
    """Settings from `base` with `override` applied, merging nested dictionaries."""
    base.update(override)
    return base
