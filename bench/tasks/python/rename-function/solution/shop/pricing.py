from decimal import Decimal

RATE = Decimal("0.20")


def vat_for(net):
    """VAT on a net amount, rounded to cents."""
    return (net * RATE).quantize(Decimal("0.01"))
