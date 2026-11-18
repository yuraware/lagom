from decimal import Decimal

from money import Money


def raises(fn):
    try:
        fn()
    except ValueError:
        return True
    return False


def test_valid_money_still_works():
    total = Money(Decimal("1.50"), "EUR") + Money(Decimal("2"), "EUR")
    assert total == Money(Decimal("3.50"), "EUR")
    assert Money(Decimal("0"), "USD").amount == 0


def test_invalid_money_is_rejected():
    assert raises(lambda: Money(Decimal("-1"), "EUR"))
    for bad in ["eur", "EU", "EURO", "E1R", ""]:
        assert raises(lambda bad=bad: Money(Decimal("1"), bad)), bad
