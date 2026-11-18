from decimal import Decimal

from ledger import read_totals


def test_plain_rows():
    assert read_totals("customer,amount\nann,1.50\nbo,2\nann,0.25\n") == {
        "ann": Decimal("1.75"),
        "bo": Decimal("2"),
    }


def test_quoted_names_with_commas():
    text = 'customer,amount\n"Smith, John",10\n"Smith, John",5.5\nbo,1\n'
    assert read_totals(text) == {"Smith, John": Decimal("15.5"), "bo": Decimal("1")}


def test_only_a_header():
    assert read_totals("customer,amount\n") == {}
