def moving_average(values, window):
    """Average of every run of `window` consecutive values, in order."""
    if window < 1:
        raise ValueError("window must be at least 1")
    return [sum(values[i:i + window]) / window for i in range(len(values) - window)]
