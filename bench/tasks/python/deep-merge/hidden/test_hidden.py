import copy

from settings import deep_merge


def test_nested_dictionaries_are_merged():
    base = {"db": {"host": "localhost", "port": 5432}, "debug": False}
    override = {"db": {"port": 6543}, "debug": True}
    assert deep_merge(base, override) == {"db": {"host": "localhost", "port": 6543}, "debug": True}


def test_inputs_are_not_modified():
    base = {"a": {"b": {"c": 1}}, "list": [1]}
    override = {"a": {"b": {"d": 2}}, "e": 3}
    before = copy.deepcopy((base, override))
    merged = deep_merge(base, override)
    assert merged == {"a": {"b": {"c": 1, "d": 2}}, "list": [1], "e": 3}
    assert (base, override) == before
    merged["a"]["b"]["c"] = 99
    assert base["a"]["b"]["c"] == 1


def test_a_value_replaces_a_dictionary():
    assert deep_merge({"a": {"b": 1}}, {"a": 5}) == {"a": 5}
