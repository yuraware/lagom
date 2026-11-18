use duration::parse_duration;

#[test]
fn valid_durations() {
    assert_eq!(parse_duration("10s"), Ok(10));
    assert_eq!(parse_duration("2m"), Ok(120));
    assert_eq!(parse_duration("1h"), Ok(3600));
}

#[test]
fn invalid_input_is_an_error_not_a_panic() {
    for bad in ["", "s", "10x", "abc", "10", "5µ", "-1s", "99999999999999999999h"] {
        let result = parse_duration(bad);
        assert!(result.is_err(), "{bad:?} gave {result:?}");
        assert!(!result.unwrap_err().is_empty());
    }
}
