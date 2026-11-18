use windows::window_sums;

#[test]
fn every_window_is_summed() {
    assert_eq!(window_sums(&[1, 2, 3, 4], 2), vec![3, 5, 7]);
    assert_eq!(window_sums(&[5], 1), vec![5]);
    assert_eq!(window_sums(&[1, 2, 3], 3), vec![6]);
    assert_eq!(window_sums(&[1, 2], 3), Vec::<i64>::new());
    assert_eq!(window_sums(&[1, 2], 0), Vec::<i64>::new());
}
