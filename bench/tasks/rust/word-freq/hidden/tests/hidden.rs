use words::word_frequencies;

fn pairs(list: &[(&str, usize)]) -> Vec<(String, usize)> {
    list.iter().map(|(w, n)| (w.to_string(), *n)).collect()
}

#[test]
fn case_and_punctuation_are_ignored() {
    let got = word_frequencies("The cat, the hat. THE end! Don't stop.");
    assert_eq!(got, pairs(&[("the", 3), ("cat", 1), ("don't", 1), ("end", 1), ("hat", 1), ("stop", 1)]));
}

#[test]
fn nothing_but_punctuation_is_no_words() {
    assert!(word_frequencies("").is_empty());
    assert!(word_frequencies("--- !!! ...").is_empty());
}
