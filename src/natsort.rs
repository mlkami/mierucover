//! Natural ("logical") ordering of file names, close to Explorer's own sort.
//!
//! Runs of digits compare by value, so `2.jpg` comes before `10.jpg`. Like Explorer,
//! punctuation sorts before digits and digits before letters, so `_001.jpg` and
//! `000.jpg` come before `000a.jpg`. Full-width digits count as digits.

use std::cmp::Ordering;

pub fn compare(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        let (c, d) = match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) => (c, d),
        };
        if digit(c).is_some() && digit(d).is_some() {
            let (m, n) = (number(&mut x), number(&mut y));
            match compare_numbers(&m, &n) {
                Ordering::Equal => continue,
                o => return o,
            }
        }
        match key(c).cmp(&key(d)) {
            Ordering::Equal => {
                x.next();
                y.next();
            }
            o => return o,
        }
    }
}

fn digit(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some(c as u8 - b'0'),
        '０'..='９' => Some((c as u32 - '０' as u32) as u8),
        _ => None,
    }
}

fn number(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Vec<u8> {
    let mut v = Vec::new();
    while let Some(d) = it.peek().copied().and_then(digit) {
        v.push(d);
        it.next();
    }
    v
}

fn compare_numbers(a: &[u8], b: &[u8]) -> Ordering {
    let strip = |v: &[u8]| -> usize { v.iter().take_while(|&&d| d == 0).count() };
    let (a, b) = (&a[strip(a)..], &b[strip(b)..]);
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// Sort key of a single character: (class, lowercase code point).
fn key(c: char) -> (u8, u32) {
    if c == '/' || c == '\\' {
        return (0, 0);
    }
    if digit(c).is_some() {
        return (2, c as u32);
    }
    if c.is_ascii() && !c.is_ascii_alphanumeric() {
        return (1, c as u32);
    }
    let lower = c.to_lowercase().next().unwrap_or(c);
    (3, lower as u32)
}

#[cfg(test)]
mod tests {
    use super::compare;

    fn sorted(names: &[&str]) -> Vec<String> {
        let mut v: Vec<String> = names.iter().map(|s| s.to_string()).collect();
        v.sort_by(|a, b| compare(a, b));
        v
    }

    #[test]
    fn numbers_by_value() {
        assert_eq!(sorted(&["10.jpg", "2.jpg", "1.jpg"]), ["1.jpg", "2.jpg", "10.jpg"]);
        assert_eq!(sorted(&["010.jpg", "01.jpg", "008.jpg"]), ["01.jpg", "008.jpg", "010.jpg"]);
    }

    #[test]
    fn punctuation_before_digits_before_letters() {
        assert_eq!(sorted(&["000a.jpg", "000.jpg"]), ["000.jpg", "000a.jpg"]);
        assert_eq!(sorted(&["001.jpg", "_001.jpg"]), ["_001.jpg", "001.jpg"]);
        assert_eq!(sorted(&["_003.png", "_.jpg"]), ["_.jpg", "_003.png"]);
        assert_eq!(sorted(&["cover.jpg", "00001.jpg"]), ["00001.jpg", "cover.jpg"]);
    }

    #[test]
    fn case_and_folders() {
        assert_eq!(sorted(&["B.jpg", "a.jpg"]), ["a.jpg", "B.jpg"]);
        assert_eq!(sorted(&["vol2/001.jpg", "vol10/001.jpg", "vol1/002.jpg"]), ["vol1/002.jpg", "vol2/001.jpg", "vol10/001.jpg"]);
    }

    #[test]
    fn full_width_digits() {
        assert_eq!(sorted(&["第１０話.jpg", "第２話.jpg"]), ["第２話.jpg", "第１０話.jpg"]);
    }
}
