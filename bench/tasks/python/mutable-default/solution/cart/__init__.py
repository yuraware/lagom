def add_item(item, cart=None):
    """Add an item to the cart (a new one if none is given) and return the cart."""
    if cart is None:
        cart = []
    cart.append(item)
    return cart
