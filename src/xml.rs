//! Just enough XML to read EPUB package files: start tags and their attributes.

pub struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
}

impl Tag {
    /// Compares the local name (without a namespace prefix), ignoring case.
    pub fn is(&self, name: &str) -> bool {
        local(&self.name).eq_ignore_ascii_case(name)
    }

    /// The value of the attribute with this local name, ignoring case and prefix.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| local(k).eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

pub fn tags(s: &str) -> Vec<Tag> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = s[i..].find('<').map(|p| i + p) {
        if s[p..].starts_with("<!--") {
            i = s[p..].find("-->").map_or(b.len(), |e| p + e + 3);
            continue;
        }
        if matches!(b.get(p + 1), Some(b'!' | b'?' | b'/') | None) {
            i = s[p..].find('>').map_or(b.len(), |e| p + e + 1);
            continue;
        }
        let mut j = p + 1;
        while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' && b[j] != b'/' {
            j += 1;
        }
        let name = s[p + 1..j].to_owned();
        let mut attrs = Vec::new();
        loop {
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= b.len() || b[j] == b'>' || b[j] == b'/' {
                break;
            }
            let k = j;
            while j < b.len() && b[j] != b'=' && b[j] != b'>' && !b[j].is_ascii_whitespace() {
                j += 1;
            }
            let key = s[k..j].to_owned();
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if b.get(j) != Some(&b'=') {
                continue;
            }
            j += 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            let value = match b.get(j) {
                Some(&q @ (b'"' | b'\'')) => {
                    let v = j + 1;
                    let e = s[v..].find(q as char).map_or(b.len(), |e| v + e);
                    j = (e + 1).min(b.len());
                    &s[v..e]
                }
                _ => {
                    let v = j;
                    while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' {
                        j += 1;
                    }
                    &s[v..j]
                }
            };
            attrs.push((key, unescape(value)));
        }
        out.push(Tag { name, attrs });
        i = s[j..].find('>').map_or(b.len(), |e| j + e + 1);
    }
    out
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let Some(e) = rest.find(';') else { break };
        let entity = &rest[1..e];
        let c = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if entity.starts_with("#x") => u32::from_str_radix(&entity[2..], 16).ok().and_then(char::from_u32),
            _ if entity.starts_with('#') => entity[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::tags;

    #[test]
    fn reads_attributes() {
        let t = tags(r#"<?xml version="1.0"?><!-- <item id="x"/> --><opf:item id='c' href="images/cover%20a.jpg" media-type="image/jpeg" properties="cover-image"/><meta name="cover" content="c" /><x a=b&amp;c>"#);
        assert_eq!(t.len(), 3);
        assert!(t[0].is("item"));
        assert_eq!(t[0].attr("href"), Some("images/cover%20a.jpg"));
        assert_eq!(t[0].attr("properties"), Some("cover-image"));
        assert_eq!(t[1].attr("content"), Some("c"));
        assert_eq!(t[2].attr("a"), Some("b&c"));
    }
}
