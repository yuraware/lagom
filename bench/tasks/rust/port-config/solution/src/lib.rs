/// The port to listen on: the value of PORT if it is set, else 8080.
pub fn port(value: Option<&str>) -> Result<u16, String> {
    let Some(v) = value else {
        return Ok(8080);
    };
    match v.parse::<u16>() {
        Ok(n) if n > 0 => Ok(n),
        _ => Err(format!("invalid port {v:?}: use a number from 1 to 65535")),
    }
}
