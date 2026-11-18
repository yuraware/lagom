/// Fibonacci numbers: 0, 1, 1, 2, 3, 5, …
pub struct Fib {
    a: u64,
    b: u64,
}

pub fn fib() -> Fib {
    Fib { a: 0, b: 1 }
}

impl Iterator for Fib {
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        let current = self.a;
        self.a = self.b;
        self.b += current;
        Some(current)
    }
}
