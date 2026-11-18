use std::collections::HashMap;

/// Word counts, most frequent first; ties in alphabetical order. Case and the punctuation
/// around a word are ignored.
pub fn word_frequencies(text: &str) -> Vec<(String, usize)> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for token in text.split_whitespace() {
        let word = token.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
        if !word.is_empty() {
            *counts.entry(word).or_default() += 1;
        }
    }
    let mut list: Vec<(String, usize)> = counts.into_iter().collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    list
}
