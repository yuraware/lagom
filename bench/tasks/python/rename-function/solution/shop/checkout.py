from shop.pricing import vat_for


def total(net):
    """What the customer pays: net plus VAT."""
    return net + vat_for(net)
