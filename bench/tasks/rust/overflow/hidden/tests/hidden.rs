use mathx::midpoint;

#[test]
fn midpoints_never_overflow() {
    assert_eq!(midpoint(0, 0), 0);
    assert_eq!(midpoint(1, 2), 1);
    assert_eq!(midpoint(3, 10), 6);
    assert_eq!(midpoint(u32::MAX, u32::MAX), u32::MAX);
    assert_eq!(midpoint(u32::MAX, u32::MAX - 1), u32::MAX - 1);
    assert_eq!(midpoint(u32::MAX, 0), u32::MAX / 2);
}
