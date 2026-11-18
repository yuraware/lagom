use geometry::Point;

#[test]
fn points_print_as_pairs() {
    assert_eq!(Point::new(3, -4).to_string(), "(3, -4)");
    assert_eq!(format!("{}", Point::new(0, 0)), "(0, 0)");
}
