from decimal import Decimal

RATE = Decimal("0.20")


def calc_tax(net):
    """VAT on a net amount, rounded to cents."""
    return (net * RATE).quantize(Decimal("0.01"))
