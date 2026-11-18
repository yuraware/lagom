from cart import add_item


def test_each_call_gets_its_own_cart():
    assert add_item("apple") == ["apple"]
    assert add_item("pear") == ["pear"]


def test_a_given_cart_is_extended():
    mine = ["milk"]
    assert add_item("bread", mine) is mine
    assert mine == ["milk", "bread"]
