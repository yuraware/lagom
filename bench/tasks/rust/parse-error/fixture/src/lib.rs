/// Parses durations like "10s", "2m" or "1h" into seconds.
pub fn parse_duration(text: &str) -> u64 {
    let (number, unit) = text.split_at(text.len() - 1);
    let n: u64 = number.parse().unwrap();
    match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        _ => panic!("unknown unit {unit}"),
    }
}
