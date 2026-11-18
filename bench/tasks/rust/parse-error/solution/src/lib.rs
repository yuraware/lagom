/// Parses durations like "10s", "2m" or "1h" into seconds.
pub fn parse_duration(text: &str) -> Result<u64, String> {
    let Some(unit) = text.chars().last() else {
        return Err("empty duration".into());
    };
    let number = &text[..text.len() - unit.len_utf8()];
    let n: u64 = number
        .parse()
        .map_err(|_| format!("not a number: {number:?}"))?;
    let factor = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        _ => return Err(format!("unknown unit {unit:?}")),
    };
    n.checked_mul(factor).ok_or_else(|| format!("{text:?} is too long"))
}
