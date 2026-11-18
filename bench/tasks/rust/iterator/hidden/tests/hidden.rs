use fib::fib;

#[test]
fn starts_right() {
    let first: Vec<u64> = fib().take(10).collect();
    assert_eq!(first, [0, 1, 1, 2, 3, 5, 8, 13, 21, 34]);
}

#[test]
fn stops_at_the_last_number_that_fits() {
    assert_eq!(fib().count(), 94);
    assert_eq!(fib().last(), Some(12_200_160_415_121_876_738));
}
