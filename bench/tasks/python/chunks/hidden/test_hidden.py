from batches import chunks


def test_partial_last_chunk_is_kept():
    assert list(chunks([1, 2, 3, 4, 5], 2)) == [[1, 2], [3, 4], [5]]
    assert list(chunks([1, 2, 3, 4], 2)) == [[1, 2], [3, 4]]
    assert list(chunks([], 3)) == []


def test_any_iterable_works():
    assert list(chunks((x * x for x in range(5)), 3)) == [[0, 1, 4], [9, 16]]
    assert list(chunks("abcde", 2)) == [["a", "b"], ["c", "d"], ["e"]]


def test_n_below_one_is_an_error():
    try:
        list(chunks([1], 0))
    except ValueError:
        return
    raise AssertionError("expected ValueError")
