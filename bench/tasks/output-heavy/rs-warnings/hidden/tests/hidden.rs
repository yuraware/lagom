use modules::*;

#[test]
fn every_function_still_works() {
    assert_eq!(m0::compute_0(5), 10);
    assert_eq!(m1::compute_1(5), 11);
    assert_eq!(m2::compute_2(5), 12);
    assert_eq!(m3::compute_3(5), 13);
    assert_eq!(m4::compute_4(5), 14);
    assert_eq!(m5::compute_5(5), 15);
    assert_eq!(m6::compute_6(5), 16);
    assert_eq!(m7::compute_7(5), 17);
    assert_eq!(m8::compute_8(5), 18);
    assert_eq!(m9::compute_9(5), 19);
    assert_eq!(m10::compute_10(5), 20);
    assert_eq!(m11::compute_11(5), 21);
}
