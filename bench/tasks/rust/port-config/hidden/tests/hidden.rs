use config::port;

#[test]
fn valid_ports_and_the_default() {
    assert_eq!(port(None), Ok(8080));
    assert_eq!(port(Some("3000")), Ok(3000));
    assert_eq!(port(Some("65535")), Ok(65535));
}

#[test]
fn invalid_ports_are_errors_that_name_the_value() {
    for bad in ["80a", "70000", "0", "", "-1"] {
        let err = port(Some(bad)).expect_err(bad);
        assert!(err.contains(&format!("{bad:?}")) || err.contains(bad), "{bad:?}: {err}");
    }
}
