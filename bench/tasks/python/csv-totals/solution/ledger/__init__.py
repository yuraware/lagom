import csv
import io
from decimal import Decimal


def read_totals(text):
    """Total amount per customer from CSV with a header line: customer,amount."""
    totals = {}
    for row in csv.DictReader(io.StringIO(text)):
        customer = row["customer"]
        totals[customer] = totals.get(customer, Decimal(0)) + Decimal(row["amount"])
    return totals
