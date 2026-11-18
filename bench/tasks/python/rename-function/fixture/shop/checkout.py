from shop.pricing import calc_tax


def total(net):
    """What the customer pays: net plus VAT."""
    return net + calc_tax(net)
