/// Fibonacci numbers: 0, 1, 1, 2, 3, 5, …, up to the last one that fits in a u64.
pub struct Fib {
    next: Option<u64>,
    after: Option<u64>,
}

pub fn fib() -> Fib {
    Fib {
        next: Some(0),
        after: Some(1),
    }
}

impl Iterator for Fib {
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        let current = self.next?;
        self.next = self.after;
        self.after = self.after.and_then(|b| b.checked_add(current));
        Some(current)
    }
}
