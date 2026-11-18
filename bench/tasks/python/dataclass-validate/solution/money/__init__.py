from dataclasses import dataclass
from decimal import Decimal


@dataclass(frozen=True)
class Money:
    amount: Decimal
    currency: str

    def __post_init__(self):
        if self.amount < 0:
            raise ValueError(f"amount can't be negative: {self.amount}")
        if not (len(self.currency) == 3 and self.currency.isalpha() and self.currency.isupper()):
            raise ValueError(f"currency must be three upper-case letters: {self.currency!r}")

    def __add__(self, other):
        if other.currency != self.currency:
            raise ValueError("currencies differ")
        return Money(self.amount + other.amount, self.currency)
