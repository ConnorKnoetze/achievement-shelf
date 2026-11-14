//! Minimal reader for Valve's text KeyValues files (`.vdf`, `.acf`).

#[derive(Debug, Clone, PartialEq)]
pub enum Vdf {
    Section(Vec<(String, Vdf)>),
    Str(String),
}

impl Vdf {
    pub fn entries(&self) -> &[(String, Vdf)] {
        match self {
            Vdf::Section(entries) => entries,
            Vdf::Str(_) => &[],
        }
    }

    pub fn get(&self, key: &str) -> Option<&Vdf> {
        self.entries()
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Vdf::Str(s) => Some(s),
            Vdf::Section(_) => None,
        }
    }
}

enum Token {
    Str(String),
    Open,
    Close,
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => tokens.push(Token::Open),
            '}' => tokens.push(Token::Close),
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(other) => s.push(other),
                            None => break,
                        },
                        other => s.push(other),
                    }
                }
                tokens.push(Token::Str(s));
            }
            c if c.is_whitespace() => {}
            c => {
                // Unquoted token: read until whitespace or a brace.
                let mut s = String::from(c);
                while let Some(&next) = chars.peek() {
                    if next.is_whitespace() || next == '{' || next == '}' || next == '"' {
                        break;
                    }
                    s.push(next);
                    chars.next();
                }
                tokens.push(Token::Str(s));
            }
        }
    }
    tokens
}

fn parse_section(tokens: &mut std::vec::IntoIter<Token>) -> Vec<(String, Vdf)> {
    let mut entries = Vec::new();
    while let Some(token) = tokens.next() {
        let key = match token {
            Token::Str(key) => key,
            Token::Close => break,
            Token::Open => continue,
        };
        match tokens.next() {
            Some(Token::Str(value)) => entries.push((key, Vdf::Str(value))),
            Some(Token::Open) => entries.push((key, Vdf::Section(parse_section(tokens)))),
            Some(Token::Close) | None => break,
        }
    }
    entries
}

/// Parses a whole file. Malformed input yields whatever could be read, never an error.
pub fn parse(text: &str) -> Vdf {
    Vdf::Section(parse_section(&mut tokenize(text).into_iter()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_appmanifest_shape() {
        let doc = parse(
            "\"AppState\"\n{\n\t\"appid\"\t\t\"228980\"\n\t\"name\"\t\t\"Steamworks \\\"Common\\\"\"\n\t\"UserConfig\"\n\t{\n\t}\n}\n",
        );
        let app = doc.get("AppState").unwrap();
        assert_eq!(app.get("appid").and_then(Vdf::as_str), Some("228980"));
        assert_eq!(app.get("name").and_then(Vdf::as_str), Some("Steamworks \"Common\""));
        assert!(app.get("userconfig").is_some());
    }
}
