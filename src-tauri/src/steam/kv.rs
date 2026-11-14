//! Reader for Valve's binary KeyValues format, used by the files in `appcache/stats`.

#[derive(Debug, Clone, PartialEq)]
pub enum Kv {
    Section(Vec<(String, Kv)>),
    Str(String),
    Int(i64),
    Float(f32),
}

impl Kv {
    /// Child entries of a section (empty for scalar values).
    pub fn entries(&self) -> &[(String, Kv)] {
        match self {
            Kv::Section(entries) => entries,
            _ => &[],
        }
    }

    /// First child with the given key (ASCII case-insensitive, as Steam is inconsistent).
    pub fn get(&self, key: &str) -> Option<&Kv> {
        self.entries()
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    pub fn path(&self, keys: &[&str]) -> Option<&Kv> {
        keys.iter().try_fold(self, |node, key| node.get(key))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Kv::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Kv::Int(n) => Some(*n),
            Kv::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
}

const T_SECTION: u8 = 0;
const T_STRING: u8 = 1;
const T_INT32: u8 = 2;
const T_FLOAT32: u8 = 3;
const T_POINTER: u8 = 4;
const T_COLOR: u8 = 6;
const T_UINT64: u8 = 7;
const T_END: u8 = 8;
const T_INT64: u8 = 10;
const T_END_ALT: u8 = 11;

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.buf.len());
        let end = end.ok_or_else(|| format!("unexpected end of data at offset {}", self.pos))?;
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn cstring(&mut self) -> Result<String, String> {
        let rest = &self.buf[self.pos..];
        let len = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| format!("unterminated string at offset {}", self.pos))?;
        let s = String::from_utf8_lossy(&rest[..len]).into_owned();
        self.pos += len + 1;
        Ok(s)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        Ok(self.take(N)?.try_into().expect("take returned N bytes"))
    }

    /// Reads entries until an end marker or (at the top level) the end of the buffer.
    fn section(&mut self, top_level: bool) -> Result<Vec<(String, Kv)>, String> {
        let mut entries = Vec::new();
        loop {
            if self.pos >= self.buf.len() {
                if top_level {
                    return Ok(entries);
                }
                return Err("section not closed before end of data".into());
            }
            let ty = self.take(1)?[0];
            if ty == T_END || ty == T_END_ALT {
                return Ok(entries);
            }
            let key = self.cstring()?;
            let value = match ty {
                T_SECTION => Kv::Section(self.section(false)?),
                T_STRING => Kv::Str(self.cstring()?),
                T_INT32 | T_POINTER | T_COLOR => Kv::Int(i32::from_le_bytes(self.array()?) as i64),
                T_FLOAT32 => Kv::Float(f32::from_le_bytes(self.array()?)),
                T_UINT64 => Kv::Int(u64::from_le_bytes(self.array()?) as i64),
                T_INT64 => Kv::Int(i64::from_le_bytes(self.array()?)),
                other => {
                    return Err(format!("unsupported value type {other} at offset {}", self.pos - 1))
                }
            };
            entries.push((key, value));
        }
    }
}

pub fn parse(buf: &[u8]) -> Result<Kv, String> {
    let mut reader = Reader { buf, pos: 0 };
    Ok(Kv::Section(reader.section(true)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_sections_and_scalars() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"\x00root\x00");
        buf.extend_from_slice(b"\x01name\x00hello\x00");
        buf.extend_from_slice(b"\x02num\x00");
        buf.extend_from_slice(&(-5i32).to_le_bytes());
        buf.extend_from_slice(b"\x00inner\x00\x02x\x00");
        buf.extend_from_slice(&7i32.to_le_bytes());
        buf.extend_from_slice(b"\x08\x08\x08");

        let kv = parse(&buf).unwrap();
        assert_eq!(kv.path(&["root", "name"]).and_then(Kv::as_str), Some("hello"));
        assert_eq!(kv.path(&["ROOT", "num"]).and_then(Kv::as_int), Some(-5));
        assert_eq!(kv.path(&["root", "inner", "x"]).and_then(Kv::as_int), Some(7));
    }

    #[test]
    fn rejects_truncated_data() {
        assert!(parse(b"\x00root\x00\x02num\x00\x01").is_err());
    }
}
