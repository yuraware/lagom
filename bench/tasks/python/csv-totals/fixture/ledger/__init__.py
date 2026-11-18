from decimal import Decimal


def read_totals(text):
    """Total amount per customer from CSV with a header line: customer,amount."""
    totals = {}
    for line in text.strip().splitlines()[1:]:
        customer, amount = line.split(",")
        totals[customer] = totals.get(customer, Decimal(0)) + Decimal(amount)
    return totals
