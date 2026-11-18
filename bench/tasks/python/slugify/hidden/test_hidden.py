from slugs import slugify


def test_plain_titles():
    assert slugify("Hello World") == "hello-world"
    assert slugify("  Python 3.12 is out  ") == "python-3-12-is-out"


def test_accents_and_punctuation():
    assert slugify("Crème Brûlée!") == "creme-brulee"
    assert slugify("Ärger -- über   Straßen?") == "arger-uber-straen"


def test_nothing_left():
    assert slugify("!!!") == ""
