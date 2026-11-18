/// Sums of every run of `k` consecutive values, in order. Empty when `k` is 0 or larger
/// than the input.
pub fn window_sums(values: &[i64], k: usize) -> Vec<i64> {
    if k == 0 || k > values.len() {
        return Vec::new();
    }
    (0..=values.len() - k).map(|i| values[i..i + k].iter().sum()).collect()
}
