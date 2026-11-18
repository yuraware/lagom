from dataclasses import dataclass
from decimal import Decimal


@dataclass(frozen=True)
class Money:
    amount: Decimal
    currency: str

    def __add__(self, other):
        if other.currency != self.currency:
            raise ValueError("currencies differ")
        return Money(self.amount + other.amount, self.currency)
