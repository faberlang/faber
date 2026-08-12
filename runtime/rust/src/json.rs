//! Object-rooted JSON document carrier for Faber `json`.

use crate::Valor;
use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct Json(Valor);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    path: String,
    kind: JsonErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonErrorKind {
    ExpectedObjectRoot,
    UnsupportedVariant(&'static str),
    NonFiniteNumber,
    DuplicateKey(String),
    InvalidSyntax(String),
    InvalidNumber(String),
    TrailingCharacters,
}

impl Json {
    /// Parse a JSON wire string into a `Json` value.
    ///
    /// # Errors
    ///
    /// Returns `Err` on invalid JSON syntax, non-finite numbers, unsupported
    /// value variants, duplicate keys, or trailing characters after the root.
    pub fn parse(wire: &str) -> Result<Self, JsonError> {
        let mut parser = Parser::new(wire);
        let value = parser.parse_value("$")?;
        parser.skip_ws();
        if !parser.is_eof() {
            return Err(JsonError::new("$", JsonErrorKind::TrailingCharacters));
        }
        Self::try_from(value)
    }

    /// Create a `Json` value from a map of object fields.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any field value is a non-finite number or an unsupported
    /// variant (`Octeti` or `Instans`).
    pub fn from_object(fields: BTreeMap<String, Valor>) -> Result<Self, JsonError> {
        Self::try_from(Valor::Tabula(fields))
    }

    #[must_use]
    pub fn as_valor(&self) -> &Valor {
        &self.0
    }

    #[must_use]
    pub fn as_object(&self) -> &BTreeMap<String, Valor> {
        let Valor::Tabula(fields) = &self.0 else {
            unreachable!("Json is validated with an object root");
        };
        fields
    }

    #[must_use]
    pub fn into_valor(self) -> Valor {
        self.0
    }

    #[must_use]
    pub fn to_wire(&self) -> String {
        render_valor(&self.0)
    }
}

impl TryFrom<Valor> for Json {
    type Error = JsonError;

    fn try_from(value: Valor) -> Result<Self, Self::Error> {
        validate_root(&value)?;
        Ok(Self(value))
    }
}

impl TryFrom<&Valor> for Json {
    type Error = JsonError;

    fn try_from(value: &Valor) -> Result<Self, Self::Error> {
        validate_root(value)?;
        Ok(Self(value.clone()))
    }
}

impl From<Json> for Valor {
    fn from(value: Json) -> Self {
        value.into_valor()
    }
}

impl JsonError {
    fn new(path: impl Into<String>, kind: JsonErrorKind) -> Self {
        Self {
            path: path.into(),
            kind,
        }
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    #[must_use]
    pub fn kind(&self) -> &JsonErrorKind {
        &self.kind
    }
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}", self.kind, self.path)
    }
}

impl std::error::Error for JsonError {}

impl fmt::Display for JsonErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedObjectRoot => write!(f, "expected JSON object root"),
            Self::UnsupportedVariant(name) => write!(f, "unsupported JSON value variant {name}"),
            Self::NonFiniteNumber => write!(f, "non-finite JSON number"),
            Self::DuplicateKey(key) => write!(f, "duplicate JSON object key {key:?}"),
            Self::InvalidSyntax(message) => write!(f, "invalid JSON syntax: {message}"),
            Self::InvalidNumber(token) => write!(f, "invalid JSON number {token:?}"),
            Self::TrailingCharacters => write!(f, "trailing characters after JSON document"),
        }
    }
}

fn validate_root(value: &Valor) -> Result<(), JsonError> {
    match value {
        Valor::Tabula(_) => validate_value(value, "$"),
        _ => Err(JsonError::new("$", JsonErrorKind::ExpectedObjectRoot)),
    }
}

fn validate_value(value: &Valor, path: &str) -> Result<(), JsonError> {
    match value {
        Valor::Nihil | Valor::Bivalens(_) | Valor::Numerus(_) | Valor::Textus(_) => Ok(()),
        Valor::Fractus(n) if n.is_finite() => Ok(()),
        Valor::Fractus(_) => Err(JsonError::new(path, JsonErrorKind::NonFiniteNumber)),
        Valor::Lista(items) => items
            .iter()
            .enumerate()
            .try_for_each(|(idx, item)| validate_value(item, &format!("{path}[{idx}]"))),
        Valor::Tabula(fields) => fields
            .iter()
            .try_for_each(|(key, value)| validate_value(value, &object_path(path, key))),
        Valor::Octeti(_) => Err(JsonError::new(
            path,
            JsonErrorKind::UnsupportedVariant("octeti"),
        )),
        Valor::Instans(_) => Err(JsonError::new(
            path,
            JsonErrorKind::UnsupportedVariant("instans"),
        )),
    }
}

fn object_path(parent: &str, key: &str) -> String {
    if key
        .chars()
        .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
    {
        format!("{parent}.{key}")
    } else {
        format!("{parent}[{}]", render_string(key))
    }
}

fn render_valor(value: &Valor) -> String {
    match value {
        Valor::Nihil => "null".to_owned(),
        Valor::Bivalens(true) => "true".to_owned(),
        Valor::Bivalens(false) => "false".to_owned(),
        Valor::Numerus(value) => value.to_string(),
        Valor::Fractus(value) => render_fractus(*value),
        Valor::Textus(value) => render_string(value),
        Valor::Lista(items) => {
            let body = items.iter().map(render_valor).collect::<Vec<_>>().join(",");
            format!("[{body}]")
        }
        Valor::Tabula(fields) => {
            let body = fields
                .iter()
                .map(|(key, value)| format!("{}:{}", render_string(key), render_valor(value)))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
        Valor::Octeti(_) | Valor::Instans(_) => {
            unreachable!("Json validation rejects unsupported Valor variants")
        }
    }
}

fn render_fractus(value: f64) -> String {
    debug_assert!(value.is_finite());
    let rendered = value.to_string();
    if rendered.contains('.') || rendered.contains('e') || rendered.contains('E') {
        rendered
    } else {
        format!("{rendered}.0")
    }
}

fn render_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

struct Parser<'a> {
    wire: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(wire: &'a str) -> Self {
        Self { wire, pos: 0 }
    }

    fn parse_value(&mut self, path: &str) -> Result<Valor, JsonError> {
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.parse_object(path),
            Some(b'[') => self.parse_array(path),
            Some(b'"') => self.parse_string(path).map(Valor::Textus),
            Some(b't') => self.parse_literal("true", Valor::Bivalens(true), path),
            Some(b'f') => self.parse_literal("false", Valor::Bivalens(false), path),
            Some(b'n') => self.parse_literal("null", Valor::Nihil, path),
            Some(b'-' | b'0'..=b'9') => self.parse_number(path),
            Some(other) => Err(self.syntax(path, format!("unexpected byte 0x{other:02x}"))),
            None => Err(self.syntax(path, "unexpected end of input")),
        }
    }

    fn parse_object(&mut self, path: &str) -> Result<Valor, JsonError> {
        self.expect(b'{', path)?;
        let mut fields = BTreeMap::new();
        self.skip_ws();
        if self.consume(b'}') {
            return Ok(Valor::Tabula(fields));
        }

        loop {
            self.skip_ws();
            let key = self.parse_string(path)?;
            self.skip_ws();
            self.expect(b':', path)?;
            let child_path = object_path(path, &key);
            let value = self.parse_value(&child_path)?;
            if fields.insert(key.clone(), value).is_some() {
                return Err(JsonError::new(child_path, JsonErrorKind::DuplicateKey(key)));
            }
            self.skip_ws();
            if self.consume(b'}') {
                return Ok(Valor::Tabula(fields));
            }
            self.expect(b',', path)?;
        }
    }

    fn parse_array(&mut self, path: &str) -> Result<Valor, JsonError> {
        self.expect(b'[', path)?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.consume(b']') {
            return Ok(Valor::Lista(items));
        }

        loop {
            let child_path = format!("{path}[{}]", items.len());
            items.push(self.parse_value(&child_path)?);
            self.skip_ws();
            if self.consume(b']') {
                return Ok(Valor::Lista(items));
            }
            self.expect(b',', path)?;
        }
    }

    fn parse_string(&mut self, path: &str) -> Result<String, JsonError> {
        self.expect(b'"', path)?;
        let mut out = String::new();
        loop {
            let Some(ch) = self.next_char() else {
                return Err(self.syntax(path, "unterminated string"));
            };
            match ch {
                '"' => return Ok(out),
                '\\' => out.push(self.parse_escape(path)?),
                ch if ch.is_control() => {
                    return Err(self.syntax(path, "unescaped control character in string"));
                }
                ch => out.push(ch),
            }
        }
    }

    fn parse_escape(&mut self, path: &str) -> Result<char, JsonError> {
        let Some(ch) = self.next_char() else {
            return Err(self.syntax(path, "unterminated escape"));
        };
        match ch {
            '"' | '\\' | '/' => Ok(ch),
            'b' => Ok('\u{08}'),
            'f' => Ok('\u{0c}'),
            'n' => Ok('\n'),
            'r' => Ok('\r'),
            't' => Ok('\t'),
            'u' => self.parse_unicode_escape(path),
            _ => Err(self.syntax(path, format!("invalid escape \\{ch}"))),
        }
    }

    fn parse_unicode_escape(&mut self, path: &str) -> Result<char, JsonError> {
        let high = self.parse_hex_quad(path)?;
        if !(0xd800..=0xdbff).contains(&high) {
            return char::from_u32(u32::from(high))
                .ok_or_else(|| self.syntax(path, "invalid unicode scalar"));
        }

        let save = self.pos;
        if self.next_char() != Some('\\') || self.next_char() != Some('u') {
            self.pos = save;
            return Err(self.syntax(path, "high surrogate without low surrogate"));
        }

        let low = self.parse_hex_quad(path)?;
        if !(0xdc00..=0xdfff).contains(&low) {
            return Err(self.syntax(path, "high surrogate without low surrogate"));
        }
        let scalar = 0x10000 + (u32::from(high - 0xd800) << 10) + u32::from(low - 0xdc00);
        char::from_u32(scalar).ok_or_else(|| self.syntax(path, "invalid unicode scalar"))
    }

    fn parse_hex_quad(&mut self, path: &str) -> Result<u16, JsonError> {
        let mut value = 0u16;
        for _ in 0..4 {
            let Some(ch) = self.next_char() else {
                return Err(self.syntax(path, "short unicode escape"));
            };
            let Some(digit) = ch.to_digit(16) else {
                return Err(self.syntax(path, "invalid unicode escape"));
            };
            // SAFETY: digit is 0..=15 from to_digit(16), safe for u16.
            #[allow(clippy::cast_possible_truncation)]
            let digit = digit as u16;
            value = (value << 4) | digit;
        }
        Ok(value)
    }

    fn parse_literal(
        &mut self,
        expected: &str,
        value: Valor,
        path: &str,
    ) -> Result<Valor, JsonError> {
        if self.wire[self.pos..].starts_with(expected) {
            self.pos += expected.len();
            Ok(value)
        } else {
            Err(self.syntax(path, format!("expected {expected}")))
        }
    }

    fn parse_number(&mut self, path: &str) -> Result<Valor, JsonError> {
        let start = self.pos;
        self.consume(b'-');
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(self.invalid_number(path, start));
                }
            }
            Some(b'1'..=b'9') => self.take_digits(),
            _ => return Err(self.invalid_number(path, start)),
        }

        let mut fractional = false;
        if self.consume(b'.') {
            fractional = true;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.invalid_number(path, start));
            }
            self.take_digits();
        }

        if self.consume(b'e') || self.consume(b'E') {
            fractional = true;
            let _ = self.consume(b'+') || self.consume(b'-');
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.invalid_number(path, start));
            }
            self.take_digits();
        }

        let token = &self.wire[start..self.pos];
        if fractional {
            let value = token
                .parse::<f64>()
                .map_err(|_| JsonError::new(path, JsonErrorKind::InvalidNumber(token.into())))?;
            if value.is_finite() {
                Ok(Valor::Fractus(value))
            } else {
                Err(JsonError::new(
                    path,
                    JsonErrorKind::InvalidNumber(token.into()),
                ))
            }
        } else {
            token
                .parse::<i64>()
                .map(Valor::Numerus)
                .map_err(|_| JsonError::new(path, JsonErrorKind::InvalidNumber(token.into())))
        }
    }

    fn take_digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.pos += 1;
        }
    }

    fn is_eof(&self) -> bool {
        self.pos == self.wire.len()
    }

    fn peek(&self) -> Option<u8> {
        self.wire.as_bytes().get(self.pos).copied()
    }

    fn consume(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, byte: u8, path: &str) -> Result<(), JsonError> {
        if self.consume(byte) {
            Ok(())
        } else {
            Err(self.syntax(path, format!("expected {}", byte as char)))
        }
    }

    fn next_char(&mut self) -> Option<char> {
        let ch = self.wire[self.pos..].chars().next()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn syntax(&self, path: &str, message: impl Into<String>) -> JsonError {
        JsonError::new(path, JsonErrorKind::InvalidSyntax(message.into()))
    }

    fn invalid_number(&self, path: &str, start: usize) -> JsonError {
        let end = self.pos.min(self.wire.len());
        JsonError::new(
            path,
            JsonErrorKind::InvalidNumber(self.wire[start..end].to_owned()),
        )
    }
}
