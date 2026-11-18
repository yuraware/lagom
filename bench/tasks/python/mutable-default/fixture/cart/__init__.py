def add_item(item, cart=[]):
    """Add an item to the cart (a new one if none is given) and return the cart."""
    cart.append(item)
    return cart
