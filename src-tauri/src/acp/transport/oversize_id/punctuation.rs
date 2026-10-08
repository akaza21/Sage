use super::{Container, Expect, MAX_DEPTH, NumberState, OversizeIdScanner, Token};

impl OversizeIdScanner {
    pub(super) fn feed_punctuation(&mut self, byte: u8) -> Result<(), ()> {
        if matches!(byte, b' ' | b'\t') {
            return Ok(());
        }
        match self.expect {
            Expect::KeyOrEnd | Expect::Key => match byte {
                b'}' if matches!(self.expect, Expect::KeyOrEnd) => self.close(Container::Object)?,
                b'"' => self.start_string(true),
                _ => return Err(()),
            },
            Expect::Colon => {
                if byte != b':' {
                    return Err(());
                }
                self.expect = Expect::Value;
            }
            Expect::ArrayValueOrEnd if byte == b']' => self.close(Container::Array)?,
            Expect::Value | Expect::ArrayValueOrEnd => match byte {
                b'{' => self.open(Container::Object)?,
                b'[' => self.open(Container::Array)?,
                b'"' if !self.key_is_id => self.start_string(false),
                b'-' | b'0'..=b'9' => {
                    let id = self.key_is_id;
                    if id && (self.id.is_some() || byte == b'-') {
                        return Err(());
                    }
                    let state = match byte {
                        b'-' => NumberState::Sign,
                        b'0' => NumberState::Zero,
                        _ => NumberState::Integer,
                    };
                    let value = if byte.is_ascii_digit() {
                        u64::from(byte - b'0')
                    } else {
                        0
                    };
                    self.token = Token::Number { state, id, value };
                }
                b't' if !self.key_is_id => {
                    self.token = Token::Literal {
                        bytes: b"true",
                        index: 1,
                    }
                }
                b'f' if !self.key_is_id => {
                    self.token = Token::Literal {
                        bytes: b"false",
                        index: 1,
                    }
                }
                b'n' if !self.key_is_id => {
                    self.token = Token::Literal {
                        bytes: b"null",
                        index: 1,
                    }
                }
                _ => return Err(()),
            },
            Expect::CommaOrEnd => match (self.stack.last(), byte) {
                (Some(Container::Object), b',') => self.expect = Expect::Key,
                (Some(Container::Array), b',') => self.expect = Expect::Value,
                (Some(Container::Object), b'}') => self.close(Container::Object)?,
                (Some(Container::Array), b']') => self.close(Container::Array)?,
                _ => return Err(()),
            },
            Expect::Done => return Err(()),
        }
        Ok(())
    }

    fn open(&mut self, kind: Container) -> Result<(), ()> {
        if self.key_is_id || self.stack.len() >= MAX_DEPTH {
            return Err(());
        }
        self.stack.push(kind);
        self.expect = match kind {
            Container::Object => Expect::KeyOrEnd,
            Container::Array => Expect::ArrayValueOrEnd,
        };
        Ok(())
    }

    fn close(&mut self, kind: Container) -> Result<(), ()> {
        if !matches!(
            (self.stack.pop(), kind),
            (Some(Container::Object), Container::Object)
                | (Some(Container::Array), Container::Array)
        ) {
            return Err(());
        }
        self.finish_value();
        Ok(())
    }

    pub(super) fn finish_value(&mut self) {
        self.key_is_id = false;
        self.expect = if self.stack.is_empty() {
            Expect::Done
        } else {
            Expect::CommaOrEnd
        };
    }

    fn start_string(&mut self, key: bool) {
        self.token = Token::String {
            key,
            match_index: 0,
            escaped: false,
            unicode_digits: 0,
            unicode_value: 0,
        };
    }
}
