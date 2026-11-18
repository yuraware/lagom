def retry(times, exceptions=(Exception,)):
    """Call the decorated function up to `times` times while it raises `exceptions`."""
    raise NotImplementedError
