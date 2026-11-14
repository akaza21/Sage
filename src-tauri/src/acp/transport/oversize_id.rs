mod punctuation;
mod utf8;
use utf8::Utf8Validator;

const ID_WINDOW: usize = 4 * 1024;
const MAX_DEPTH: usize = 32;

#[derive(Clone, Copy)]
enum Container {
    Object,
    Array,
}

#[derive(Clone, Copy)]
enum Expect {
    Value,
    ArrayValueOrEnd,
    KeyOrEnd,
    Key,
    Colon,
    CommaOrEnd,
    Done,
}

#[derive(Clone, Copy)]
enum NumberState {
    Sign,
    Zero,
    Integer,
    Dot,
    Fraction,
    Exponent,
    ExponentSign,
    ExponentDigits,
}

#[derive(Clone, Copy)]
enum Token {
    None,
    String {
        key: bool,
        match_index: u8,
        escaped: bool,
        unicode_digits: u8,
        unicode_value: u16,
    },
    Number {
        state: NumberState,
        id: bool,
        value: u64,
    },
    Literal {
        bytes: &'static [u8],
        index: usize,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Scanned {
    Reply(u64),
    /// One complete value with no top-level id; nothing waits on it.
    Idless,
    /// Malformed, or a request that a peer waits on.
    Invalid,
}

pub(super) struct OversizeIdScanner {
    stack: Vec<Container>,
    expect: Expect,
    token: Token,
    key_is_id: bool,
    id: Option<u64>,
    /// A top-level `method` key: the frame is a request or notification,
    /// so its id never answers one of Sage's requests.
    has_method: bool,
    bytes_seen: usize,
    utf8: Utf8Validator,
}

impl OversizeIdScanner {
    pub(super) fn new() -> Self {
        Self {
            stack: Vec::new(),
            expect: Expect::Value,
            token: Token::None,
            key_is_id: false,
            id: None,
            has_method: false,
            bytes_seen: 0,
            utf8: Utf8Validator::new(),
        }
    }

    pub(super) fn feed(&mut self, byte: u8) -> Result<(), ()> {
        self.bytes_seen += 1;
        let mut retry = true;
        while retry {
            retry = false;
            match self.token {
                Token::String {
                    key,
                    match_index,
                    escaped,
                    unicode_digits,
                    unicode_value,
                } => {
                    if !self.utf8.is_complete() {
                        self.utf8.feed(byte)?;
                        self.token = Token::String {
                            key,
                            match_index: NO_KEY,
                            escaped,
                            unicode_digits,
                            unicode_value,
                        };
                    } else if unicode_digits != 0 {
                        let digit = (byte as char).to_digit(16).ok_or(())? as u16;
                        let value = (unicode_value << 4) | digit;
                        if unicode_digits == 1 {
                            if (0xd800..=0xdfff).contains(&value) {
                                return Err(());
                            }
                            let next = if key {
                                match_key(match_index, value)
                            } else {
                                match_index
                            };
                            self.token = Token::String {
                                key,
                                match_index: next,
                                escaped: false,
                                unicode_digits: 0,
                                unicode_value: 0,
                            };
                        } else {
                            self.token = Token::String {
                                key,
                                match_index,
                                escaped,
                                unicode_digits: unicode_digits - 1,
                                unicode_value: value,
                            };
                        }
                    } else if escaped {
                        let next = match byte {
                            b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {
                                if key {
                                    match_key(match_index, unescaped(byte))
                                } else {
                                    match_index
                                }
                            }
                            b'u' => {
                                self.token = Token::String {
                                    key,
                                    match_index,
                                    escaped: false,
                                    unicode_digits: 4,
                                    unicode_value: 0,
                                };
                                return Ok(());
                            }
                            _ => return Err(()),
                        };
                        self.token = Token::String {
                            key,
                            match_index: next,
                            escaped: false,
                            unicode_digits: 0,
                            unicode_value: 0,
                        };
                    } else {
                        match byte {
                            b'"' => {
                                self.token = Token::None;
                                if key {
                                    let top = self.stack.len() == 1;
                                    self.key_is_id = top && match_index == ID_KEY;
                                    self.has_method |= top && match_index == METHOD_KEY;
                                    self.expect = Expect::Colon;
                                } else {
                                    self.finish_value();
                                }
                            }
                            b'\\' => {
                                self.token = Token::String {
                                    key,
                                    match_index,
                                    escaped: true,
                                    unicode_digits: 0,
                                    unicode_value: 0,
                                }
                            }
                            0..=0x1f => return Err(()),
                            _ => {
                                self.utf8.feed(byte)?;
                                let next = if key {
                                    match_key(match_index, u16::from(byte))
                                } else {
                                    match_index
                                };
                                self.token = Token::String {
                                    key,
                                    match_index: next,
                                    escaped: false,
                                    unicode_digits: 0,
                                    unicode_value: 0,
                                };
                            }
                        }
                    }
                }
                Token::Number { state, id, value } => {
                    let next = match (state, byte) {
                        (NumberState::Sign, b'0') => Some(NumberState::Zero),
                        (NumberState::Sign, b'1'..=b'9') => Some(NumberState::Integer),
                        (NumberState::Zero | NumberState::Integer, b'.') => Some(NumberState::Dot),
                        (NumberState::Integer, b'0'..=b'9') => Some(NumberState::Integer),
                        (NumberState::Dot | NumberState::Fraction, b'0'..=b'9') => {
                            Some(NumberState::Fraction)
                        }
                        (
                            NumberState::Zero | NumberState::Integer | NumberState::Fraction,
                            b'e' | b'E',
                        ) => Some(NumberState::Exponent),
                        (NumberState::Exponent, b'+' | b'-') => Some(NumberState::ExponentSign),
                        (
                            NumberState::Exponent
                            | NumberState::ExponentSign
                            | NumberState::ExponentDigits,
                            b'0'..=b'9',
                        ) => Some(NumberState::ExponentDigits),
                        _ => None,
                    };
                    if let Some(next) = next {
                        let value = if id {
                            if !matches!(next, NumberState::Zero | NumberState::Integer) {
                                return Err(());
                            }
                            value
                                .checked_mul(10)
                                .and_then(|n| n.checked_add(u64::from(byte - b'0')))
                                .ok_or(())?
                        } else {
                            value
                        };
                        self.token = Token::Number {
                            state: next,
                            id,
                            value,
                        };
                    } else {
                        if !matches!(
                            state,
                            NumberState::Zero
                                | NumberState::Integer
                                | NumberState::Fraction
                                | NumberState::ExponentDigits
                        ) {
                            return Err(());
                        }
                        if id
                            && (self.bytes_seen - 1 > ID_WINDOW
                                || value > i64::MAX as u64
                                || self.id.replace(value).is_some())
                        {
                            return Err(());
                        }
                        self.token = Token::None;
                        self.finish_value();
                        retry = true;
                    }
                }
                Token::Literal { bytes, index } => {
                    if bytes.get(index) != Some(&byte) {
                        return Err(());
                    }
                    if index + 1 == bytes.len() {
                        self.token = Token::None;
                        self.finish_value();
                    } else {
                        self.token = Token::Literal {
                            bytes,
                            index: index + 1,
                        };
                    }
                }
                Token::None => self.feed_punctuation(byte)?,
            }
        }
        Ok(())
    }

    /// A frame naming a `method` is a request or
    /// a notification, never a reply, whatever its id.
    pub(super) fn finish(&self) -> Scanned {
        if !self.is_complete() {
            return Scanned::Invalid;
        }
        match (self.id, self.has_method) {
            (Some(id), false) => Scanned::Reply(id),
            (None, _) => Scanned::Idless,
            (Some(_), true) => Scanned::Invalid,
        }
    }

    fn is_complete(&self) -> bool {
        matches!(self.token, Token::None)
            && matches!(self.expect, Expect::Done)
            && self.stack.is_empty()
            && self.utf8.is_complete()
    }
}

fn unescaped(letter: u8) -> u16 {
    u16::from(match letter {
        b'b' => 0x08,
        b'f' => 0x0c,
        b'n' => b'\n',
        b'r' => b'\r',
        b't' => b'\t',
        other => other,
    })
}

/// Key-matching states: a key so far reads `i`, `id`, a prefix of
/// `method`, or none of them (`NO_KEY`, which never leaves).
const ID_KEY: u8 = 2;
const NO_KEY: u8 = 3;
const METHOD_KEY: u8 = 9;

fn match_key(index: u8, value: u16) -> u8 {
    match (index, value) {
        (0, 0x69) => 1,
        (1, 0x64) => ID_KEY,
        (0, 0x6d) => 4,
        (4, 0x65) => 5,
        (5, 0x74) => 6,
        (6, 0x68) => 7,
        (7, 0x6f) => 8,
        (8, 0x64) => METHOD_KEY,
        _ => NO_KEY,
    }
}
