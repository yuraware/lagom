//! Byte-exact edits of string values inside a JSON document.
//!
//! The proxy shrinks tool output inside API request bodies. Parsing a body and
//! serializing it again would change spacing, key order, number spelling and
//! escapes in the part of the request the provider has already cached, and that
//! breaks the prompt cache. This module never re-serializes:
//!
//! - [`locate`] scans the body once and returns the byte range of each value
//!   asked for by path, for example `messages[12].content[0].content`.
//! - [`splice`] replaces chosen string values with new text, escaped, and keeps
//!   every other byte of the body as it was.
//!
//! The scanner checks the whole body against the JSON grammar (RFC 8259) and
//! refuses anything else, so on any error the caller forwards the original
//! body unchanged (fail open).

use std::fmt;

/// Deepest container nesting accepted. Deeper input is refused rather than
/// risking the stack on a hostile body.
pub const MAX_DEPTH: usize = 256;

/// One step of a path: an object key or an array index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seg {
    Key(String),
    Index(usize),
}

/// The JSON type of a located value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Object,
    Array,
    String,
    Number,
    Bool,
    Null,
}

/// Byte range `start..end` of one value in the body. For a string it includes
/// both quotes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

impl Span {
    /// The value's bytes exactly as they appear in `body`.
    pub fn bytes<'a>(&self, body: &'a [u8]) -> &'a [u8] {
        &body[self.start..self.end]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpliceError {
    /// The body is not valid JSON.
    Invalid { offset: usize, reason: &'static str },
    /// Containers nest deeper than [`MAX_DEPTH`].
    TooDeep { offset: usize },
    /// A path string could not be parsed.
    BadPath(String),
    /// Path number `index` matched more than one value (duplicate object keys).
    Ambiguous { index: usize },
    /// Path number `index` matched nothing.
    Missing { index: usize },
    /// Edit number `index` does not point at a string value in this body.
    NotString { index: usize },
    /// Two edits cover the same bytes.
    Overlap,
}

impl fmt::Display for SpliceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid { offset, reason } => {
                write!(f, "invalid JSON at byte {offset}: {reason}")
            }
            Self::TooDeep { offset } => {
                write!(
                    f,
                    "JSON nests deeper than {MAX_DEPTH} levels at byte {offset}"
                )
            }
            Self::BadPath(msg) => write!(f, "bad path: {msg}"),
            Self::Ambiguous { index } => write!(f, "path {index} matches more than one value"),
            Self::Missing { index } => write!(f, "path {index} matches no value"),
            Self::NotString { index } => write!(f, "edit {index} is not a string value"),
            Self::Overlap => write!(f, "edits overlap"),
        }
    }
}

impl std::error::Error for SpliceError {}

/// Parse a path such as `messages[12].content[0].content`.
///
/// - `name` or `.name` is an object key. Keys can't contain `.`, `[` or `]`
///   in this form; write those as `["a.b"]`, a JSON string in brackets.
/// - `[N]` is an array index.
/// - The empty path is the top-level value.
pub fn parse_path(path: &str) -> Result<Vec<Seg>, SpliceError> {
    let b = path.as_bytes();
    let bad = |msg: &str| SpliceError::BadPath(format!("{msg} in {path:?}"));
    let mut segs = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'[' if b.get(i + 1) == Some(&b'"') => {
                let mut sc = Scanner::new(b);
                sc.pos = i + 1;
                sc.string().map_err(|_| bad("unterminated quoted key"))?;
                let key: String = serde_json::from_slice(&b[i + 1..sc.pos])
                    .map_err(|_| bad("invalid quoted key"))?;
                if b.get(sc.pos) != Some(&b']') {
                    return Err(bad("expected ] after quoted key"));
                }
                segs.push(Seg::Key(key));
                i = sc.pos + 1;
            }
            b'[' => {
                let close = b[i..]
                    .iter()
                    .position(|&c| c == b']')
                    .ok_or_else(|| bad("missing ]"))?;
                let digits = &path[i + 1..i + close];
                if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
                    return Err(bad("index must be digits"));
                }
                let n = digits.parse().map_err(|_| bad("index too large"))?;
                segs.push(Seg::Index(n));
                i += close + 1;
            }
            b'.' if !segs.is_empty() => {
                i += 1;
                let len = key_len(&b[i..]);
                if len == 0 {
                    return Err(bad("empty key"));
                }
                segs.push(Seg::Key(path[i..i + len].to_string()));
                i += len;
            }
            _ if segs.is_empty() && i == 0 => {
                let len = key_len(b);
                if len == 0 {
                    return Err(bad("empty key"));
                }
                segs.push(Seg::Key(path[..len].to_string()));
                i = len;
            }
            _ => return Err(bad("expected . or [")),
        }
    }
    Ok(segs)
}

fn key_len(b: &[u8]) -> usize {
    b.iter()
        .position(|c| matches!(c, b'.' | b'[' | b']'))
        .unwrap_or(b.len())
}

/// Find the value at each path. Result `i` is `None` when path `i` matches
/// nothing. The whole body is checked, even after every path is found, so
/// invalid JSON is always refused.
pub fn locate<P: AsRef<[Seg]>>(body: &[u8], paths: &[P]) -> Result<Vec<Option<Span>>, SpliceError> {
    if let Err(e) = std::str::from_utf8(body) {
        return Err(SpliceError::Invalid {
            offset: e.valid_up_to(),
            reason: "not UTF-8",
        });
    }
    let paths: Vec<&[Seg]> = paths.iter().map(|p| p.as_ref()).collect();
    let mut sc = Scanner::new(body);
    let mut found = vec![None; paths.len()];
    let all: Vec<usize> = (0..paths.len()).collect();
    sc.ws();
    sc.value(0, &all, &paths, &mut found)?;
    sc.ws();
    if sc.pos != body.len() {
        return Err(sc.invalid("trailing characters after the value"));
    }
    Ok(found)
}

/// Replace string values with new text. Each edit is a [`Span`] from
/// [`locate`] on this same `body` and the new, unescaped text. Bytes outside
/// the edited spans are copied unchanged.
pub fn splice(body: &[u8], edits: &[(Span, &str)]) -> Result<Vec<u8>, SpliceError> {
    for (index, (span, _)) in edits.iter().enumerate() {
        let is_string_literal = span.kind == Kind::String
            && span.start < span.end
            && span.end - span.start >= 2
            && span.end <= body.len()
            && body[span.start] == b'"'
            && body[span.end - 1] == b'"';
        if !is_string_literal {
            return Err(SpliceError::NotString { index });
        }
    }
    let mut order: Vec<usize> = (0..edits.len()).collect();
    order.sort_by_key(|&i| edits[i].0.start);
    if order
        .windows(2)
        .any(|w| edits[w[0]].0.end > edits[w[1]].0.start)
    {
        return Err(SpliceError::Overlap);
    }
    let added: usize = edits.iter().map(|(_, text)| text.len() + 2).sum();
    let mut out = Vec::with_capacity(body.len() + added);
    let mut copied = 0;
    for i in order {
        let (span, text) = edits[i];
        out.extend_from_slice(&body[copied..span.start]);
        write_string(text, &mut out);
        copied = span.end;
    }
    out.extend_from_slice(&body[copied..]);
    Ok(out)
}

/// Locate the string at each path and replace it in one call. Every path must
/// match a string value.
pub fn splice_paths(body: &[u8], edits: &[(&str, &str)]) -> Result<Vec<u8>, SpliceError> {
    let paths = edits
        .iter()
        .map(|(p, _)| parse_path(p))
        .collect::<Result<Vec<_>, _>>()?;
    let spans = locate(body, &paths)?;
    let mut resolved = Vec::with_capacity(edits.len());
    for (index, (span, (_, text))) in spans.into_iter().zip(edits).enumerate() {
        let span = span.ok_or(SpliceError::Missing { index })?;
        resolved.push((span, *text));
    }
    splice(body, &resolved)
}

/// Decode the string value at `span` into its text. A span that is not a
/// string gives `NotString { index: 0 }`.
pub fn decode_string(body: &[u8], span: Span) -> Result<String, SpliceError> {
    if span.kind != Kind::String || span.end > body.len() || span.start >= span.end {
        return Err(SpliceError::NotString { index: 0 });
    }
    serde_json::from_slice(span.bytes(body)).map_err(|_| SpliceError::Invalid {
        offset: span.start,
        reason: "string cannot be decoded",
    })
}

/// Append `text` as a JSON string literal, quotes included. The escaping
/// matches `serde_json`: `"`, `\` and control characters are escaped, and
/// everything else, including non-ASCII, is written as is.
pub fn write_string(text: &str, out: &mut Vec<u8>) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push(b'"');
    let bytes = text.as_bytes();
    let mut run = 0;
    for (i, &c) in bytes.iter().enumerate() {
        let short = match c {
            b'"' => b'"',
            b'\\' => b'\\',
            b'\n' => b'n',
            b'\r' => b'r',
            b'\t' => b't',
            0x08 => b'b',
            0x0c => b'f',
            0x00..=0x1f => b'u',
            _ => continue,
        };
        out.extend_from_slice(&bytes[run..i]);
        out.push(b'\\');
        out.push(short);
        if short == b'u' {
            out.extend_from_slice(&[b'0', b'0', HEX[(c >> 4) as usize], HEX[(c & 0xf) as usize]]);
        }
        run = i + 1;
    }
    out.extend_from_slice(&bytes[run..]);
    out.push(b'"');
}

struct Scanner<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Scanner<'a> {
    fn new(b: &'a [u8]) -> Self {
        Scanner { b, pos: 0 }
    }

    fn invalid(&self, reason: &'static str) -> SpliceError {
        SpliceError::Invalid {
            offset: self.pos,
            reason,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, c: u8, reason: &'static str) -> Result<(), SpliceError> {
        if self.peek() != Some(c) {
            return Err(self.invalid(reason));
        }
        self.pos += 1;
        Ok(())
    }

    /// Scan one value at path depth `depth`. `active` holds the paths whose
    /// first `depth` steps lead here.
    fn value(
        &mut self,
        depth: usize,
        active: &[usize],
        paths: &[&[Seg]],
        found: &mut [Option<Span>],
    ) -> Result<(), SpliceError> {
        let start = self.pos;
        let kind = match self.peek() {
            Some(b'{') => {
                self.object(depth, active, paths, found)?;
                Kind::Object
            }
            Some(b'[') => {
                self.array(depth, active, paths, found)?;
                Kind::Array
            }
            Some(b'"') => {
                self.string()?;
                Kind::String
            }
            Some(b'-' | b'0'..=b'9') => {
                self.number()?;
                Kind::Number
            }
            Some(b't') => {
                self.literal(b"true")?;
                Kind::Bool
            }
            Some(b'f') => {
                self.literal(b"false")?;
                Kind::Bool
            }
            Some(b'n') => {
                self.literal(b"null")?;
                Kind::Null
            }
            None => return Err(self.invalid("unexpected end of input")),
            Some(_) => return Err(self.invalid("expected a value")),
        };
        for &i in active {
            if paths[i].len() == depth {
                if found[i].is_some() {
                    return Err(SpliceError::Ambiguous { index: i });
                }
                found[i] = Some(Span {
                    start,
                    end: self.pos,
                    kind,
                });
            }
        }
        Ok(())
    }

    fn enter(&self, depth: usize) -> Result<(), SpliceError> {
        if depth >= MAX_DEPTH {
            return Err(SpliceError::TooDeep { offset: self.pos });
        }
        Ok(())
    }

    fn object(
        &mut self,
        depth: usize,
        active: &[usize],
        paths: &[&[Seg]],
        found: &mut [Option<Span>],
    ) -> Result<(), SpliceError> {
        self.enter(depth)?;
        self.pos += 1;
        self.ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(());
        }
        let mut child = Vec::new();
        loop {
            if self.peek() != Some(b'"') {
                return Err(self.invalid("expected a string key"));
            }
            let key_start = self.pos;
            self.string()?;
            let key = &self.b[key_start..self.pos];
            child.clear();
            if !active.is_empty() {
                let decoded = decode_key(key);
                child.extend(active.iter().copied().filter(|&i| {
                    matches!(paths[i].get(depth), Some(Seg::Key(k)) if decoded.as_deref() == Some(k.as_bytes()))
                }));
            }
            self.ws();
            self.expect(b':', "expected : after key")?;
            self.ws();
            self.value(depth + 1, &child, paths, found)?;
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    self.ws();
                }
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => return Err(self.invalid("expected , or } in object")),
            }
        }
    }

    fn array(
        &mut self,
        depth: usize,
        active: &[usize],
        paths: &[&[Seg]],
        found: &mut [Option<Span>],
    ) -> Result<(), SpliceError> {
        self.enter(depth)?;
        self.pos += 1;
        self.ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(());
        }
        let mut child = Vec::new();
        let mut index = 0;
        loop {
            child.clear();
            child.extend(
                active
                    .iter()
                    .copied()
                    .filter(|&i| paths[i].get(depth) == Some(&Seg::Index(index))),
            );
            self.value(depth + 1, &child, paths, found)?;
            index += 1;
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    self.ws();
                }
                Some(b']') => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => return Err(self.invalid("expected , or ] in array")),
            }
        }
    }

    /// Scan a string literal starting at the opening quote.
    fn string(&mut self) -> Result<(), SpliceError> {
        self.pos += 1;
        loop {
            match self.peek() {
                None => return Err(self.invalid("unterminated string")),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(());
                }
                Some(b'\\') => {
                    self.pos += 1;
                    match self.peek() {
                        Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => {
                            self.pos += 1
                        }
                        Some(b'u') => {
                            let hex = self.b.get(self.pos + 1..self.pos + 5);
                            if !hex.is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)) {
                                return Err(self.invalid("\\u needs four hex digits"));
                            }
                            self.pos += 5;
                        }
                        _ => return Err(self.invalid("invalid escape")),
                    }
                }
                Some(0x00..=0x1f) => return Err(self.invalid("control character in string")),
                Some(_) => self.pos += 1,
            }
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        self.pos - start
    }

    fn number(&mut self) -> Result<(), SpliceError> {
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(self.invalid("leading zero in number"));
                }
            }
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(self.invalid("expected a digit")),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if self.digits() == 0 {
                return Err(self.invalid("expected a digit after ."));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return Err(self.invalid("expected a digit in exponent"));
            }
        }
        Ok(())
    }

    fn literal(&mut self, word: &[u8]) -> Result<(), SpliceError> {
        if !self.b[self.pos..].starts_with(word) {
            return Err(self.invalid("invalid literal"));
        }
        self.pos += word.len();
        Ok(())
    }
}

/// The decoded bytes of a key literal (quotes included), borrowing when it has
/// no escapes. `None` if it can't be decoded, such as a lone surrogate escape;
/// such a key matches no path.
fn decode_key(raw: &[u8]) -> Option<std::borrow::Cow<'_, [u8]>> {
    let inner = &raw[1..raw.len() - 1];
    if !inner.contains(&b'\\') {
        return Some(std::borrow::Cow::Borrowed(inner));
    }
    serde_json::from_slice::<String>(raw)
        .ok()
        .map(|s| std::borrow::Cow::Owned(s.into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn path(p: &str) -> Vec<Seg> {
        parse_path(p).unwrap()
    }

    fn find(body: &str, p: &str) -> Option<Span> {
        locate(body.as_bytes(), &[path(p)]).unwrap()[0]
    }

    #[test]
    fn parses_paths() {
        use Seg::{Index, Key};
        assert_eq!(path(""), vec![]);
        assert_eq!(
            path("messages[12].content[0].content"),
            vec![
                Key("messages".into()),
                Index(12),
                Key("content".into()),
                Index(0),
                Key("content".into())
            ]
        );
        assert_eq!(path("[3][0]"), vec![Index(3), Index(0)]);
        assert_eq!(
            path(r#"a["b.c"]["q\"\u00e9[]"].d"#),
            vec![
                Key("a".into()),
                Key("b.c".into()),
                Key("q\"é[]".into()),
                Key("d".into())
            ]
        );
        assert_eq!(path("tool_use-id é"), vec![Key("tool_use-id é".into())]);
        for bad in [
            "a..b",
            "a.",
            ".a",
            "a[",
            "a[]",
            "a[x]",
            "a[-1]",
            "a]",
            r#"a["b"#,
            r#"a["b"x"#,
            "a[1]b",
        ] {
            assert!(
                matches!(parse_path(bad), Err(SpliceError::BadPath(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn locates_values_by_path_and_kind() {
        let body = r#" {"messages":[{"role":"user","content":[{"type":"tool_result","content":"out\nput","n":-1.5e3,"ok":true,"x":null}]}],"m":{}} "#;
        let s = find(body, "messages[0].content[0].content").unwrap();
        assert_eq!(s.kind, Kind::String);
        assert_eq!(s.bytes(body.as_bytes()), br#""out\nput""#);
        assert_eq!(decode_string(body.as_bytes(), s).unwrap(), "out\nput");
        assert_eq!(
            find(body, "messages[0].content[0].n").unwrap().kind,
            Kind::Number
        );
        assert_eq!(
            find(body, "messages[0].content[0].ok").unwrap().kind,
            Kind::Bool
        );
        assert_eq!(
            find(body, "messages[0].content[0].x").unwrap().kind,
            Kind::Null
        );
        assert_eq!(find(body, "messages[0].content").unwrap().kind, Kind::Array);
        assert_eq!(find(body, "m").unwrap().kind, Kind::Object);
        let root = find(body, "").unwrap();
        assert_eq!(
            (root.start, root.end, root.kind),
            (1, body.len() - 1, Kind::Object)
        );
        for missing in [
            "messages[1]",
            "messages[0].content[0].nope",
            "m.a",
            "messages.a",
            "messages[0].role[0]",
            "[0]",
        ] {
            assert_eq!(find(body, missing), None, "{missing}");
        }
    }

    #[test]
    fn matches_keys_written_with_escapes() {
        let body = r#"{"t\u0079pe":"a","c\/d":"b"}"#;
        assert_eq!(
            find(body, "type").unwrap().bytes(body.as_bytes()),
            br#""a""#
        );
        assert_eq!(find(body, "c/d").unwrap().bytes(body.as_bytes()), br#""b""#);
        // A key that only decodes to a lone surrogate matches nothing but is still valid JSON.
        assert_eq!(find(r#"{"\ud800":"x"}"#, "a"), None);
    }

    #[test]
    fn splices_only_the_chosen_strings() {
        let body =
            "{\n  \"a\" : \"old\\u0041\",\t\"keep\": [1.50, 2E+3 ,\"\\/x\"],\n  \"b\":\"two\"\n}\n";
        let out =
            splice_paths(body.as_bytes(), &[("b", "new \"q\"\n\u{1}é😀"), ("a", "")]).unwrap();
        let expected = "{\n  \"a\" : \"\",\t\"keep\": [1.50, 2E+3 ,\"\\/x\"],\n  \"b\":\"new \\\"q\\\"\\n\\u0001é😀\"\n}\n";
        assert_eq!(String::from_utf8(out).unwrap(), expected);
    }

    #[test]
    fn refuses_bad_edits() {
        let body = br#"{"a":"x","n":1}"#;
        let spans = locate(body, &[path("a"), path("n")]).unwrap();
        let (a, n) = (spans[0].unwrap(), spans[1].unwrap());
        assert_eq!(
            splice(body, &[(n, "y")]),
            Err(SpliceError::NotString { index: 0 })
        );
        assert_eq!(
            splice(body, &[(a, "y"), (a, "z")]),
            Err(SpliceError::Overlap)
        );
        let past_end = Span {
            start: 5,
            end: 99,
            kind: Kind::String,
        };
        assert_eq!(
            splice(body, &[(past_end, "y")]),
            Err(SpliceError::NotString { index: 0 })
        );
        let not_quoted = Span {
            start: 2,
            end: 5,
            kind: Kind::String,
        };
        assert_eq!(
            splice(body, &[(not_quoted, "y")]),
            Err(SpliceError::NotString { index: 0 })
        );
        let huge = Span {
            start: usize::MAX - 1,
            end: usize::MAX,
            kind: Kind::String,
        };
        assert_eq!(
            splice(body, &[(huge, "y")]),
            Err(SpliceError::NotString { index: 0 })
        );
        assert_eq!(splice(body, &[]).unwrap(), body.to_vec());
        assert_eq!(
            splice_paths(body, &[("b", "y")]),
            Err(SpliceError::Missing { index: 0 })
        );
        assert!(matches!(
            splice_paths(body, &[("a..", "y")]),
            Err(SpliceError::BadPath(_))
        ));
    }

    #[test]
    fn duplicate_keys_are_ambiguous() {
        let body = br#"{"a":"x","a":"y"}"#;
        assert_eq!(
            locate(body, &[path("a")]),
            Err(SpliceError::Ambiguous { index: 0 })
        );
        // Duplicates that no path asks for are fine.
        assert!(locate(body, &[path("b")]).is_ok());
    }

    #[test]
    fn refuses_invalid_json() {
        let cases = [
            "",
            " ",
            "{",
            "[1,]",
            "{\"a\":1,}",
            "{\"a\" 1}",
            "{a:1}",
            "[01]",
            "[1.]",
            "[.5]",
            "[1e]",
            "[-]",
            "[+1]",
            "[\"\\x\"]",
            "[\"\\u12g4\"]",
            "[\"a\tb\"]",
            "[\"abc]",
            "tru",
            "nul",
            "[true false]",
            "{} {}",
            "\u{feff}{}",
            "[1]x",
            "{\"a\":1 \"b\":2}",
            "'a'",
            "[NaN]",
        ];
        for case in cases {
            assert!(
                matches!(
                    locate(case.as_bytes(), &[path("")]),
                    Err(SpliceError::Invalid { .. })
                ),
                "{case:?}"
            );
            assert!(
                serde_json::from_str::<Value>(case).is_err(),
                "serde accepts {case:?}"
            );
        }
        assert!(matches!(
            locate(b"[\"\xff\"]", &[path("")]),
            Err(SpliceError::Invalid { offset: 2, .. })
        ));
    }

    #[test]
    fn caps_nesting_depth() {
        let nested = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
        assert!(locate(nested(MAX_DEPTH).as_bytes(), &[path("")]).is_ok());
        assert!(matches!(
            locate(nested(MAX_DEPTH + 1).as_bytes(), &[path("")]),
            Err(SpliceError::TooDeep { .. })
        ));
        // A huge hostile depth is refused without overflowing the stack.
        assert!(locate("[".repeat(1_000_000).as_bytes(), &[path("")]).is_err());
    }

    #[test]
    fn handles_a_large_body() {
        let big = "x\\n".repeat(1 << 20);
        let body =
            format!(r#"{{"system":"s","messages":[{{"content":"{big}"}},{{"content":"keep"}}]}}"#);
        let out = splice_paths(body.as_bytes(), &[("messages[0].content", "short")]).unwrap();
        assert_eq!(
            out,
            br#"{"system":"s","messages":[{"content":"short"},{"content":"keep"}]}"#
        );
    }

    // ---- Property tests with a seeded generator (no extra dependency). ----

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            // xorshift64*
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
        fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
            &items[self.below(items.len())]
        }
    }

    const CHARS: &[char] = &[
        'a', 'b', 'Z', '0', ' ', '_', '"', '\\', '/', '\n', '\t', '\r', '\u{1}', '\u{8}', '\u{c}',
        '\u{1f}', '\u{7f}', 'é', '€', '😀', '\u{2028}',
    ];

    fn gen_text(rng: &mut Rng) -> String {
        (0..rng.below(12)).map(|_| *rng.pick(CHARS)).collect()
    }

    /// Write `text` as a JSON literal, picking among the valid spellings of
    /// each character at random.
    fn write_messy(rng: &mut Rng, text: &str, out: &mut String) {
        out.push('"');
        for c in text.chars() {
            let short = match c {
                '"' => Some("\\\""),
                '\\' => Some("\\\\"),
                '/' => Some("\\/"),
                '\n' => Some("\\n"),
                '\t' => Some("\\t"),
                '\r' => Some("\\r"),
                '\u{8}' => Some("\\b"),
                '\u{c}' => Some("\\f"),
                _ => None,
            };
            let must_escape = c == '"' || c == '\\' || (c as u32) < 0x20;
            match (rng.below(3), short) {
                (0, Some(s)) => out.push_str(s),
                (1, _) => {
                    let mut units = [0u16; 2];
                    for u in c.encode_utf16(&mut units) {
                        out.push_str(&if rng.below(2) == 0 {
                            format!("\\u{u:04x}")
                        } else {
                            format!("\\u{u:04X}")
                        });
                    }
                }
                _ if must_escape => out.push_str(&format!("\\u{:04x}", c as u32)),
                _ => out.push(c),
            }
        }
        out.push('"');
    }

    fn ws(rng: &mut Rng, out: &mut String) {
        out.push_str(rng.pick(&["", "", " ", "\n  ", "\t", "\r\n"]));
    }

    fn gen_value(rng: &mut Rng, depth: usize, out: &mut String) {
        let choice = if depth >= 5 {
            2 + rng.below(4)
        } else {
            rng.below(7)
        };
        match choice {
            0 | 6 => {
                out.push('{');
                let n = rng.below(5);
                for i in 0..n {
                    ws(rng, out);
                    // "k{i}_" keeps decoded keys unique within the object.
                    let key = format!("k{i}_{}", gen_text(rng));
                    write_messy(rng, &key, out);
                    ws(rng, out);
                    out.push(':');
                    ws(rng, out);
                    gen_value(rng, depth + 1, out);
                    ws(rng, out);
                    if i + 1 < n {
                        out.push(',');
                    }
                }
                out.push('}');
            }
            1 => {
                out.push('[');
                let n = rng.below(5);
                for i in 0..n {
                    ws(rng, out);
                    gen_value(rng, depth + 1, out);
                    ws(rng, out);
                    if i + 1 < n {
                        out.push(',');
                    }
                }
                out.push(']');
            }
            2 | 3 => {
                let text = gen_text(rng);
                write_messy(rng, &text, out);
            }
            4 => out.push_str(rng.pick(&[
                "0",
                "-0",
                "-1",
                "3.25",
                "1e5",
                "-0.5E-3",
                "2E+10",
                "12345678901234567890",
            ])),
            _ => out.push_str(rng.pick(&["true", "false", "null"])),
        }
    }

    fn string_paths(v: &Value, prefix: &mut Vec<Seg>, acc: &mut Vec<Vec<Seg>>) {
        match v {
            Value::String(_) => acc.push(prefix.clone()),
            Value::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    prefix.push(Seg::Index(i));
                    string_paths(item, prefix, acc);
                    prefix.pop();
                }
            }
            Value::Object(map) => {
                for (k, item) in map {
                    prefix.push(Seg::Key(k.clone()));
                    string_paths(item, prefix, acc);
                    prefix.pop();
                }
            }
            _ => {}
        }
    }

    fn at_mut<'v>(v: &'v mut Value, p: &[Seg]) -> &'v mut Value {
        p.iter().fold(v, |v, seg| match seg {
            Seg::Key(k) => &mut v[k.as_str()],
            Seg::Index(i) => &mut v[*i],
        })
    }

    #[test]
    fn property_splice_keeps_every_other_byte() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut edited = 0;
        for _ in 0..3000 {
            let mut doc = String::new();
            ws(&mut rng, &mut doc);
            gen_value(&mut rng, 0, &mut doc);
            ws(&mut rng, &mut doc);
            let body = doc.as_bytes();
            let original: Value =
                serde_json::from_str(&doc).unwrap_or_else(|e| panic!("{e}: {doc}"));

            let mut all = Vec::new();
            string_paths(&original, &mut Vec::new(), &mut all);
            let chosen: Vec<Vec<Seg>> = all.into_iter().filter(|_| rng.below(2) == 0).collect();
            let spans = locate(body, &chosen).unwrap_or_else(|e| panic!("{e}: {doc}"));

            let mut expected = original.clone();
            let mut texts = Vec::new();
            for (p, span) in chosen.iter().zip(&spans) {
                let span = span.unwrap_or_else(|| panic!("{p:?} not found in {doc}"));
                assert_eq!(span.kind, Kind::String);
                // The located span decodes to the value serde_json sees at that path.
                assert_eq!(
                    Value::String(decode_string(body, span).unwrap()),
                    *at_mut(&mut original.clone(), p)
                );
                let text = gen_text(&mut rng);
                *at_mut(&mut expected, p) = Value::String(text.clone());
                texts.push(text);
            }
            let edits: Vec<(Span, &str)> = spans
                .iter()
                .map(|s| s.unwrap())
                .zip(texts.iter().map(String::as_str))
                .collect();
            let out = splice(body, &edits).unwrap();

            // 1. The result parses to the expected value.
            let parsed: Value = serde_json::from_slice(&out).unwrap();
            assert_eq!(parsed, expected, "{doc}");

            // 2. Outside the edits every byte is identical, and each edit is
            //    exactly serde_json's spelling of the new text.
            let mut sorted = edits.clone();
            sorted.sort_by_key(|(s, _)| s.start);
            let (mut bi, mut oi) = (0, 0);
            for (span, text) in &sorted {
                let gap = span.start - bi;
                assert_eq!(&out[oi..oi + gap], &body[bi..span.start], "{doc}");
                oi += gap;
                let literal = serde_json::to_vec(text).unwrap();
                assert_eq!(&out[oi..oi + literal.len()], &literal[..], "{doc}");
                oi += literal.len();
                bi = span.end;
            }
            assert_eq!(&out[oi..], &body[bi..], "{doc}");
            edited += edits.len();
        }
        assert!(edited > 1000, "too few edits exercised: {edited}");
    }

    #[test]
    fn property_escaping_matches_serde_json() {
        let mut rng = Rng(7);
        for _ in 0..5000 {
            let text = gen_text(&mut rng);
            let mut out = Vec::new();
            write_string(&text, &mut out);
            assert_eq!(out, serde_json::to_vec(&text).unwrap(), "{text:?}");
        }
        let all_controls: String = (0u8..0x20).map(char::from).collect();
        let mut out = Vec::new();
        write_string(&all_controls, &mut out);
        assert_eq!(out, serde_json::to_vec(&all_controls).unwrap());
    }

    #[test]
    fn property_mutated_input_never_panics_and_agrees_with_serde_json() {
        let mut rng = Rng(42);
        let alphabet = b"{}[]:,\"\\ 0123456789.eE+-tfnrulsa/u\n\x01\xc3\xa9";
        let (mut valid, mut invalid) = (0, 0);
        for _ in 0..3000 {
            let mut doc = String::new();
            gen_value(&mut rng, 0, &mut doc);
            let mut bytes = doc.into_bytes();
            for _ in 0..1 + rng.below(3) {
                let at = rng.below(bytes.len() + 1);
                match rng.below(3) {
                    0 if at < bytes.len() => bytes[at] = *rng.pick(alphabet),
                    1 if at < bytes.len() => {
                        bytes.remove(at);
                    }
                    _ => bytes.insert(at, *rng.pick(alphabet)),
                }
            }
            let ours = locate(&bytes, &[path(""), path("k0_"), path("[0]")]);
            let theirs = serde_json::from_slice::<Value>(&bytes);
            // The grammar allows numbers too large for an f64 and lone
            // surrogate escapes (a mutation can split a `\uD83D\uDE00` pair).
            // serde_json refuses both; the splicer reads neither, so it accepts.
            let surrogate_escape = bytes.windows(4).any(|w| {
                w[0] == b'\\'
                    && w[1] == b'u'
                    && w[2].eq_ignore_ascii_case(&b'd')
                    && matches!(w[3].to_ascii_lowercase(), b'8'..=b'9' | b'a'..=b'f')
            });
            let out_of_range = theirs
                .as_ref()
                .is_err_and(|e| e.to_string().contains("number out of range"));
            if surrogate_escape || out_of_range {
                continue;
            }
            // A mutation can also duplicate a key that a path asks for.
            let ours_ok = matches!(ours, Ok(_) | Err(SpliceError::Ambiguous { .. }));
            assert_eq!(
                ours_ok,
                theirs.is_ok(),
                "{:?}: {ours:?}",
                String::from_utf8_lossy(&bytes)
            );
            if ours_ok {
                valid += 1;
            } else {
                invalid += 1;
            }
        }
        assert!(
            valid > 300 && invalid > 300,
            "{valid} valid, {invalid} invalid"
        );
    }
}
