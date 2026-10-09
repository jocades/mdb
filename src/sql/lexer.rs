use std::{fmt, ops};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[rustfmt::skip]
pub enum Kind {
    Ident, Number, String,

    Plus, Minus,
    Star, Slash,

    Eq, BangEq,
    Gt, GtEq,
    Lt, LtEq,

    Comma, Semi,
    LParen, RParen,


    Create, Drop, Table,
    Select, From, Where,
    Insert, Into, Values,

    True, False,
    And, Or,

    Eof, // MUST stay last: COUNT is derived from it
}

impl Kind {
    pub const COUNT: usize = Kind::Eof as usize + 1;

    pub fn from_u8(n: u8) -> Kind {
        assert!((n as usize) < Self::COUNT);
        // SAFETY: Kind is #[repr(u8)] and fieldless with no explicit
        // discriminants, so its values are exactly 0..COUNT, and n is in range.
        unsafe { std::mem::transmute(n) }
    }
}

const _: () = assert!(Kind::COUNT <= 64);

fn lookup_ident(lexeme: &str) -> Kind {
    match_case_insensitive!(lexeme,
        "create" => Kind::Create, "drop" => Kind::Drop, "table" => Kind::Table,
        "select" => Kind::Select, "from" => Kind::From, "where" => Kind::Where,
        "insert" => Kind::Insert, "into" => Kind::Into, "values" => Kind::Values,
        "true" => Kind::True, "false" => Kind::False,
        "and" => Kind::And, "or" => Kind::Or,
        _ => Kind::Ident,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl From<ops::Range<usize>> for Span {
    fn from(range: ops::Range<usize>) -> Self {
        Self {
            start: range.start,
            end: range.end,
        }
    }
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (self.start..self.end).fmt(f)
    }
}

#[rustfmt::skip]
impl ariadne::Span for Span {
    type SourceId = ();
    fn source(&self) -> &Self::SourceId { &() }
    fn start(&self) -> usize { self.start }
    fn end(&self) -> usize { self.end }
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum Error {
    UnexpectedCharacter { ch: char, at: usize },
    UnterminatedString { span: Span },
}

pub struct Lexer<'a> {
    src: &'a [u8],
    start: usize,
    cursor: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            src: src.as_bytes(),
            start: 0,
            cursor: 0,
        }
    }

    pub fn spanned(self) -> SpannedIter<'a> {
        SpannedIter { lexer: self }
    }

    pub fn span(&self) -> Span {
        (self.start..self.cursor).into()
    }

    pub fn lexeme(&self) -> &'a str {
        unsafe { str::from_utf8_unchecked(&self.src[self.start..self.cursor]) }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.cursor).copied()
    }

    // fn peek2(&self) -> Option<u8> {
    //     self.src.get(self.cursor + 1).copied()
    // }

    fn bump(&mut self, n: usize) {
        self.cursor += n;
    }

    fn eat(&mut self) -> Option<u8> {
        self.peek().inspect(|_| self.bump(1))
    }

    fn seek(&mut self, pred: impl Fn(u8) -> bool) {
        while self.peek().is_some_and(&pred) {
            self.bump(1);
        }
    }

    fn matches(&mut self, ch: u8) -> bool {
        if self.peek() == Some(ch) {
            self.bump(1);
            return true;
        }
        false
    }

    fn skip_whitespace(&mut self) {
        self.seek(|ch| ch.is_ascii_whitespace())
    }

    fn scan(&mut self) -> Result<Option<Kind>, Error> {
        self.skip_whitespace();
        self.start = self.cursor;

        let Some(ch) = self.eat() else {
            return Ok(None);
        };

        let token = match ch {
            b',' => Kind::Comma,
            b';' => Kind::Semi,
            b'(' => Kind::LParen,
            b')' => Kind::RParen,

            b'+' => Kind::Plus,
            b'-' => Kind::Minus,
            b'*' => Kind::Star,
            b'/' => Kind::Slash,

            b'=' => Kind::Eq,
            b'!' if self.matches(b'=') => Kind::BangEq,
            b'>' if self.matches(b'=') => Kind::GtEq,
            b'>' => Kind::Gt,
            b'<' if self.matches(b'=') => Kind::LtEq,
            b'<' => Kind::Lt,

            b'\'' => {
                self.seek(|c| c != b'\'');
                self.eat()
                    .ok_or_else(|| Error::UnterminatedString { span: self.span() })?;
                Kind::String
            }

            c if c.is_ascii_alphabetic() || c == b'_' => {
                self.seek(|c| c.is_ascii_alphanumeric() || c == b'_');
                lookup_ident(self.lexeme())
            }

            c if c.is_ascii_digit() => {
                self.seek(|c| c.is_ascii_digit());
                Kind::Number
            }

            _ => {
                bail!(Error::UnexpectedCharacter {
                    ch: ch as char,
                    at: self.start,
                });
            }
        };

        Ok(Some(token))
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = Result<Kind, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.scan().transpose()
    }
}

pub struct SpannedIter<'a> {
    lexer: Lexer<'a>,
}

impl<'a> Iterator for SpannedIter<'a> {
    type Item = (Result<Kind, Error>, Span);

    fn next(&mut self) -> Option<Self::Item> {
        self.lexer.next().map(|res| (res, self.lexer.span()))
    }
}
