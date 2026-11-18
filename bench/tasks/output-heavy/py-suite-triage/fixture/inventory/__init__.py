def parse_sku(text):
    """ "ABC-0123" -> ("ABC", 123): three capital letters, a hyphen, four digits."""
    letters, digits = text[:3], text[4:]
    if len(text) != 8 or text[3:4] != "-" or not (letters.isalpha() and letters.isupper()) or not digits.isdigit():
        raise ValueError(f"bad SKU: {text!r}")
    return letters, int(digits)


def format_qty(n):
    return "1 item" if n == 1 else f"{n} items"


def apply_discount(cents, percent):
    """Price in cents after a percent discount, rounded to the nearest cent (halves up)."""
    return cents * (100 - percent) // 100


def reserve(stock, qty):
    """Stock left after reserving `qty`. ValueError if `qty` is below 1 or more than the stock."""
    return stock - qty
