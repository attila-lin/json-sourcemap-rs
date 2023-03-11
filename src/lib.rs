#![doc = include_str!("../README.md")]

use std::collections::HashMap;

use once_cell::sync::Lazy;
use serde_json::{Number, Value};

const ESCAPED_CHARS: Lazy<HashMap<char, &'static str>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert('b', r"\b");
    map.insert('f', r"\f");
    map.insert('n', "\n");
    map.insert('r', "\r");
    map.insert('t', "\t");
    map.insert('"', "\"");
    map.insert('/', "/");
    map.insert('\\', "\\");
    map
});

/// The json-source-map error type
#[derive(Debug, thiserror::Error, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Error {
    #[error("Unexpected end of JSON input")]
    UnexpectedEof,
    #[error("Unexpected token: {0} in JSON at position {1}")]
    UnexpectedToken(char, usize),
    #[error("Convert to unicode codepoint failed")]
    Int,
    #[error("Invalid unicode codepoint: {0} at position {1}")]
    InvalidUnicodeCodePoint(u32, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Location {
    pub line: usize,
    pub column: usize,
    pub pos: usize,
}

/// The parse options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Options {
    /// Whether to allow big integers
    pub bigint: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Prop {
    Key,
    KeyEnd,
    Value,
    ValueEnd,
}

struct Parser {
    source: String,
    #[allow(dead_code)]
    options: Options,

    line: usize,
    column: usize,
    pos: usize,

    /// key is the json pointer, value is the start and end location
    pointers: HashMap<String, LocationMap>,
}

#[derive(Debug, Clone)]
pub struct ParseResult {
    pub value: Value,
    pub pointers: HashMap<String, LocationMap>,
}

impl ParseResult {
    /// Get the location of the json pointer
    pub fn get_location(&self, ptr: &str) -> Option<&LocationMap> {
        self.pointers.get(ptr)
    }
}

/// The location information of the json pointer
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LocationMap(HashMap<Prop, Location>);

impl LocationMap {
    /// Get the location of the property
    pub fn get(&self, prop: Prop) -> Option<Location> {
        self.0.get(&prop).cloned()
    }

    fn insert(&mut self, prop: Prop, loc: Location) {
        self.0.insert(prop, loc);
    }

    /// Get the start location of the json pointer's value
    pub fn value(&self) -> Location {
        self.get(Prop::Value).unwrap()
    }

    /// Get the start location of the json pointer's key
    pub fn key(&self) -> Location {
        self.get(Prop::Key).unwrap()
    }

    /// Get the end location of the json pointer's value
    pub fn value_end(&self) -> Location {
        self.get(Prop::ValueEnd).unwrap()
    }

    /// Get the end location of the json pointer's key
    pub fn key_end(&self) -> Location {
        self.get(Prop::KeyEnd).unwrap()
    }
}

impl Parser {
    fn new(source: &str, options: Options) -> Self {
        Parser {
            source: source.to_string(),
            options,
            line: 0,
            column: 0,
            pos: 0,
            pointers: HashMap::new(),
        }
    }

    fn parse(&mut self, ptr: &str, top_level: bool) -> Result<Value, Error> {
        self.whitespace();
        self.map(ptr, Prop::Value);
        let c = self.get_char()?;
        let data = match c {
            't' => {
                self.expect("rue")?;
                Value::Bool(true)
            }
            'f' => {
                self.expect("alse")?;
                Value::Bool(false)
            }
            'n' => {
                self.expect("ull")?;
                Value::Null
            }
            '"' => Value::String(self.parse_string()?),
            '[' => Value::Array(self.parse_array(ptr)?),
            '{' => self.parse_object(ptr)?,
            '-' | '0'..='9' => Value::Number(self.parse_number()?),
            _ => return Err(Error::UnexpectedToken(c, self.pos)),
        };
        self.map(ptr, Prop::ValueEnd);
        // dbg!("?");
        self.whitespace();
        // dbg!("? ?", top_level, self.pos, self.len());
        if top_level && self.pos < self.len() {
            return Err(self.unexpected_token());
        }

        Ok(data)
    }

    #[inline]
    fn len(&self) -> usize {
        self.source.chars().count()
    }

    fn whitespace(&mut self) {
        'outer: {
            while self.pos < self.len() {
                match self.source.chars().nth(self.pos) {
                    Some(' ') => self.column += 1,
                    Some('\t') => self.column += 4,
                    Some('\r') => self.column = 0,
                    Some('\n') => {
                        self.line += 1;
                        self.column = 0;
                    }
                    _ => break 'outer,
                }
                self.pos += 1;
            }
            // dbg!(1);
        }
    }

    fn parse_string(&mut self) -> Result<String, Error> {
        let mut s = String::new();
        loop {
            match self.get_char()? {
                '"' => break,
                '\\' => {
                    let c = self.get_char()?;
                    if let Some(escaped) = ESCAPED_CHARS.get(&c) {
                        s.push_str(escaped);
                    } else if c == 'u' {
                        s.push(self.get_char_code()?);
                    } else {
                        return Err(self.was_unexpected_token());
                    }
                }
                c @ _ => {
                    s.push(c);
                }
            }
            // dbg!(2);
        }
        Ok(s)
    }

    fn parse_number(&mut self) -> Result<serde_json::value::Number, Error> {
        self.back_char();

        let mut num_str = String::new();
        // let mut is_integer = true;
        if self.next() == '-' {
            num_str.push(self.get_char()?);
        }

        let next = if self.next() == '0' {
            self.get_char()?.to_string()
        } else {
            self.get_digits()?
        };
        num_str = num_str + &next;

        if self.next() == '.' {
            // is_integer = false;
            num_str.push(self.get_char()?);
            num_str = num_str + &self.get_digits()?;
        }

        if self.next() == 'e' || self.next() == 'E' {
            // is_integer = false;
            num_str.push(self.get_char()?);
            if self.next() == '-' || self.next() == '+' {
                num_str.push(self.get_char()?);
            }
            num_str = num_str + &self.get_digits()?;
        }

        // let res = num_str.parse::<f64>().unwrap();

        // let n = if is_integer {
        //     serde_json::number::N::PosInt(res)
        // } else {
        //     res
        // };

        Ok(Number::from_string_unchecked(num_str))
    }

    fn parse_array(&mut self, ptr: &str) -> Result<Vec<Value>, Error> {
        self.whitespace();
        let mut array = Vec::new();
        let c = self.get_char()?; // [
        if c == ']' {
            return Ok(array);
        }
        self.back_char();

        loop {
            let item_ptr = format!("{}/{}", ptr, array.len());
            array.push(self.parse(&item_ptr, false)?);
            self.whitespace();
            let c = self.get_char()?;
            if c == ']' {
                break;
            } else if c != ',' {
                return Err(self.unexpected_token());
            }
            self.whitespace();
            // dbg!(3);
        }

        Ok(array)
    }

    fn parse_object(&mut self, ptr: &str) -> Result<Value, Error> {
        self.whitespace();
        let mut object = serde_json::Map::new();
        if self.get_char()? == '}' {
            return Ok(object.into());
        }

        self.back_char();

        loop {
            let loc = self.get_location();
            if self.get_char()? != '"' {
                return Err(self.was_unexpected_token());
            }
            let key = self.parse_string()?;
            let prop_ptr = format!("{}/{}", ptr, Self::escape_json_pointer(&key));
            self.map_location(&prop_ptr, Prop::Key, loc);
            self.map(&prop_ptr, Prop::KeyEnd);
            self.whitespace();
            if self.get_char()? != ':' {
                return Err(self.was_unexpected_token());
            }
            self.whitespace();
            let value = self.parse(&prop_ptr, false)?;
            object.insert(key, value);
            self.whitespace();

            match self.get_char()? {
                '}' => break,
                ',' => {}
                _ => return Err(self.was_unexpected_token()),
            }

            self.whitespace();
        }
        Ok(object.into())
    }

    fn expect(&mut self, s: &str) -> Result<(), Error> {
        for c in s.chars() {
            if self.get_char()? != c {
                return Err(self.was_unexpected_token());
            }
        }
        Ok(())
    }

    #[inline]
    fn get_char(&mut self) -> Result<char, Error> {
        self.check_unexpected_eof()?;
        let c = self.next();
        self.pos += 1;
        self.column += 1;
        Ok(c)
    }

    #[inline]
    fn next(&self) -> char {
        self.source
            .chars()
            .nth(self.pos)
            .expect(&format!("Unexpected EOF, pos: {}", self.pos))
    }

    /// Backs up the parser one character.
    fn back_char(&mut self) {
        self.pos -= 1;
        self.column -= 1;
    }

    fn get_char_code(&mut self) -> Result<char, Error> {
        let count = 4;
        let mut code = String::new();
        for _ in 0..count {
            let c = self.get_char()?;
            if !c.is_ascii_hexdigit() {
                return Err(Error::UnexpectedToken(c, self.pos));
            }
            code.push(c);
        }

        let unicode = u32::from_str_radix(&code, 16).map_err(|_| Error::Int)?;
        char::from_u32(unicode).ok_or_else(|| Error::InvalidUnicodeCodePoint(unicode, self.pos))
    }

    fn get_digits(&mut self) -> Result<String, Error> {
        let mut digits = String::new();
        loop {
            let c = self.next();
            if c.is_ascii_digit() {
                digits.push(self.get_char()?);
            } else {
                break;
            }
            // dbg!(5);
        }
        Ok(digits)
    }

    fn map(&mut self, ptr: impl ToString, pro: Prop) {
        self.map_location(ptr, pro, self.get_location());
    }

    fn map_location(&mut self, ptr: impl ToString, prop: Prop, loc: Location) {
        self.pointers
            .entry(ptr.to_string())
            .or_insert_with(|| LocationMap(HashMap::new()))
            .insert(prop, loc);
    }

    fn get_location(&self) -> Location {
        Location {
            line: self.line,
            column: self.column,
            pos: self.pos,
        }
    }

    fn unexpected_token(&self) -> Error {
        Error::UnexpectedToken(self.next(), self.pos)
    }

    fn was_unexpected_token(&mut self) -> Error {
        self.back_char();
        self.unexpected_token()
    }

    fn check_unexpected_eof(&self) -> Result<(), Error> {
        if self.pos >= self.len() {
            return Err(Error::UnexpectedEof);
        }

        Ok(())
    }

    fn escape_json_pointer(s: &str) -> String {
        s.replace("~", "~0").replace("/", "~1")
    }
}

pub fn parse(source: &str, options: Options) -> Result<ParseResult, Error> {
    let mut parser = Parser::new(source, options);
    let value = parser.parse("", true)?;
    Ok(ParseResult {
        value,
        pointers: parser.pointers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse() {
        let source = r#"{
            "name": "John",
            "age": 30,
            "cars": [
                "Ford",
                "BMW",
                "Fiat"
            ]
        }"#;

        let res = parse(source, Options::default()).unwrap();
        assert!(res.value.is_object());
        assert_eq!(
            res.pointers["/name"].key(),
            Location {
                line: 1,
                column: 12,
                pos: 14
            }
        );
        assert_eq!(
            res.pointers["/name"].key_end(),
            Location {
                line: 1,
                column: 18,
                pos: 20
            }
        );
        assert_eq!(
            res.value,
            serde_json::from_str::<serde_json::Value>(source).unwrap()
        );

        let source = r#"{
  "foo": "bar"
}"#;
        let res = parse(source, Options::default()).unwrap();
        assert!(res.value.is_object());
        assert_eq!(
            res.pointers[""].value(),
            Location {
                line: 0,
                column: 0,
                pos: 0
            }
        );
        assert_eq!(
            res.pointers[""].value_end(),
            Location {
                line: 2,
                column: 1,
                pos: 18
            }
        );

        assert_eq!(
            res.pointers["/foo"].key(),
            Location {
                line: 1,
                column: 2,
                pos: 4
            }
        );
        assert_eq!(
            res.pointers["/foo"].key_end(),
            Location {
                line: 1,
                column: 7,
                pos: 9
            }
        );
        assert_eq!(
            res.pointers["/foo"].value(),
            Location {
                line: 1,
                column: 9,
                pos: 11
            }
        );
        assert_eq!(
            res.pointers["/foo"].value_end(),
            Location {
                line: 1,
                column: 14,
                pos: 16
            }
        );
        assert_eq!(
            res.value,
            serde_json::from_str::<serde_json::Value>(source).unwrap()
        );

        let source = r#"{
            "name": "John",
            "age": 30.0
        }"#;
        let res = parse(source, Options::default()).unwrap();
        assert!(res.value.is_object());
        assert_eq!(
            res.pointers["/age"].value(),
            Location {
                line: 2,
                column: 19,
                pos: 49
            }
        );
        assert_eq!(
            res.pointers["/age"].value_end(),
            Location {
                line: 2,
                column: 23,
                pos: 53
            }
        );
        assert_eq!(
            res.value,
            serde_json::from_str::<serde_json::Value>(source).unwrap()
        );

        let source = r#"{"number":1.23e+10000}"#;
        let res = parse(source, Options::default()).unwrap();
        assert!(res.value.is_object());
        assert_eq!(
            res.pointers["/number"].value(),
            Location {
                line: 0,
                column: 10,
                pos: 10
            }
        );
        assert_eq!(
            res.pointers["/number"].value_end(),
            Location {
                line: 0,
                column: 21,
                pos: 21
            }
        );

        let source = r#"{"number":-1.23e-10000}"#;
        let res = parse(source, Options::default()).unwrap();
        assert!(res.value.is_object());
        assert_eq!(
            res.pointers["/number"].value(),
            Location {
                line: 0,
                column: 10,
                pos: 10
            }
        );
        assert_eq!(
            res.pointers["/number"].value_end(),
            Location {
                line: 0,
                column: 22,
                pos: 22
            }
        );

        let source = r#"{"number":-0.0}"#;
        let res = parse(source, Options::default()).unwrap();
        assert!(res.value.is_object());
        assert_eq!(
            res.pointers["/number"].value(),
            Location {
                line: 0,
                column: 10,
                pos: 10
            }
        );
        assert_eq!(
            res.pointers["/number"].value_end(),
            Location {
                line: 0,
                column: 14,
                pos: 14
            }
        );
        assert_eq!(
            res.value,
            serde_json::from_str::<serde_json::Value>(source).unwrap()
        );

        let source = r#"{"code":"\u0020"}"#;
        let res = parse(source, Options::default()).unwrap();
        assert!(res.value.is_object());
        assert_eq!(
            res.pointers["/code"].value(),
            Location {
                line: 0,
                column: 8,
                pos: 8
            }
        );
        assert_eq!(
            res.pointers["/code"].value_end(),
            Location {
                line: 0,
                column: 16,
                pos: 16
            }
        );
        assert_eq!(
            res.value,
            serde_json::from_str::<serde_json::Value>(source).unwrap()
        );

        let source = r#"{"chinese":"你好"}"#;
        let res = parse(source, Options::default()).unwrap();
        assert!(res.value.is_object());
        assert_eq!(
            res.pointers["/chinese"].value(),
            Location {
                line: 0,
                column: 11,
                pos: 11
            }
        );
        assert_eq!(
            res.pointers["/chinese"].value_end(),
            Location {
                line: 0,
                column: 15,
                pos: 15
            }
        );
    }
}
