/// The port to listen on: the value of PORT if it is set, else 8080.
pub fn port(value: Option<&str>) -> u16 {
    value.and_then(|v| v.parse().ok()).unwrap_or(8080)
}
