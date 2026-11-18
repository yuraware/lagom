from retrying import retry


def flaky(failures, error=ConnectionError):
    calls = []

    def f(x):
        calls.append(x)
        if len(calls) <= failures:
            raise error("boom")
        return x * 2

    return f, calls


def test_retries_until_it_works():
    f, calls = flaky(2)
    assert retry(3, (ConnectionError,))(f)(21) == 42
    assert len(calls) == 3


def test_gives_up_and_reraises():
    f, calls = flaky(5)
    try:
        retry(3, (ConnectionError,))(f)(1)
    except ConnectionError:
        pass
    else:
        raise AssertionError("expected ConnectionError")
    assert len(calls) == 3


def test_other_errors_pass_through_at_once():
    f, calls = flaky(1, error=KeyError)
    try:
        retry(3, (ConnectionError,))(f)(1)
    except KeyError:
        pass
    assert len(calls) == 1


def test_keeps_the_name():
    @retry(2)
    def fetch():
        return 1

    assert fetch.__name__ == "fetch"
