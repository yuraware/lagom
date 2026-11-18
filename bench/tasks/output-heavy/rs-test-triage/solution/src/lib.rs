/// Sum of the decimal digits of `n`.
pub fn digit_sum(mut n: u64) -> u64 {
    let mut sum = 0;
    while n > 0 {
        sum += n % 10;
        n /= 10;
    }
    sum
}

/// Whether `year` is a leap year in the Gregorian calendar.
pub fn is_leap(year: u32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// `v` limited to 0..=100.
pub fn clamp_percent(v: i64) -> u8 {
    v.clamp(0, 100) as u8
}

/// A hexadecimal number without prefix, in either case: "ff", "FF", "1a2B".
pub fn parse_hex(s: &str) -> Option<u32> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(s, 16).ok()
}
