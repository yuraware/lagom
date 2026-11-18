/// The midpoint of two values, rounded down.
pub fn midpoint(a: u32, b: u32) -> u32 {
    ((a as u64 + b as u64) / 2) as u32
}
