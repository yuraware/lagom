import functools


def retry(times, exceptions=(Exception,)):
    """Call the decorated function up to `times` times while it raises `exceptions`."""
    def decorate(func):
        @functools.wraps(func)
        def wrapper(*args, **kwargs):
            for attempt in range(times):
                try:
                    return func(*args, **kwargs)
                except exceptions:
                    if attempt == times - 1:
                        raise
        return wrapper
    return decorate
