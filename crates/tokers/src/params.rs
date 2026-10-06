//! Ordered query parameters with the encoding the app traffic was captured with.

use std::fmt;

/// Ordered `key=value` list. [`Params::set`] replaces in place and keeps the key's
/// first position (Python dict semantics), so rebuilt URLs keep the captured order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Params(Vec<(String, String)>);

impl Params {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a raw query string. Blank values are dropped and duplicate keys keep
    /// the last value at the first position.
    pub fn parse(query: &str) -> Self {
        let mut out = Self::new();
        for (k, v) in form_urlencoded::parse(query.as_bytes()) {
            if !v.is_empty() {
                out.set(k, v);
            }
        }
        out
    }

    pub fn with(mut self, key: impl Into<String>, value: impl ToString) -> Self {
        self.set(key, value.to_string());
        self
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let (key, value) = (key.into(), value.into());
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.0.push((key, value)),
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn remove(&mut self, key: &str) -> Option<String> {
        let idx = self.0.iter().position(|(k, _)| k == key)?;
        Some(self.0.remove(idx).1)
    }

    /// `self` then `other`, `other` winning on shared keys.
    pub fn merged(&self, other: &Params) -> Params {
        let mut out = self.clone();
        for (k, v) in other.iter() {
            out.set(k, v);
        }
        out
    }

    /// The suffix starting at `key` (inclusive), or everything when absent.
    pub fn from_key(&self, key: &str) -> Params {
        let start = self.0.iter().position(|(k, _)| k == key).unwrap_or(0);
        Params(self.0[start..].to_vec())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `application/x-www-form-urlencoded` the way Python's `urlencode` does it:
    /// unreserved `A-Za-z0-9_.-~` kept, space as `+`, everything else `%XX`.
    pub fn encode(&self) -> String {
        let mut out = String::new();
        for (i, (k, v)) in self.0.iter().enumerate() {
            if i > 0 {
                out.push('&');
            }
            quote_plus(k, &mut out);
            out.push('=');
            quote_plus(v, &mut out);
        }
        out
    }
}

impl fmt::Display for Params {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.encode())
    }
}

impl<K: Into<String>, V: ToString> FromIterator<(K, V)> for Params {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut out = Params::new();
        for (k, v) in iter {
            out.set(k, v.to_string());
        }
        out
    }
}

fn quote_plus(s: &str, out: &mut String) {
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_compatible() {
        let mut p = Params::parse("a=1&b=&c=x%2Ay&a=2");
        assert_eq!(p.encode(), "a=2&c=x%2Ay");
        p.set("k", "a b*~é");
        assert_eq!(p.get("k"), Some("a b*~é"));
        assert!(p.encode().ends_with("k=a+b%2A~%C3%A9"));
    }
}
