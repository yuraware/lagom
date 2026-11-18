import re

SKU = re.compile(r"^[A-Z]{3}-\d{4}$")


def is_sku(text):
    return SKU.match(text) is not None
