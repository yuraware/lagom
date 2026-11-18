from decimal import Decimal

import shop.pricing
from shop.checkout import total
from shop.pricing import vat_for


def test_renamed_and_unchanged():
    assert vat_for(Decimal("10.00")) == Decimal("2.00")
    assert total(Decimal("9.99")) == Decimal("11.99")


def test_old_name_is_gone():
    assert not hasattr(shop.pricing, "calc_tax")
