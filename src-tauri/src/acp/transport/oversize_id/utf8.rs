pub(super) struct Utf8Validator {
    remaining: u8,
    min: u8,
    max: u8,
}

impl Utf8Validator {
    pub(super) fn new() -> Self {
        Self {
            remaining: 0,
            min: 0x80,
            max: 0xbf,
        }
    }

    pub(super) fn is_complete(&self) -> bool {
        self.remaining == 0
    }

    pub(super) fn feed(&mut self, byte: u8) -> Result<(), ()> {
        if self.remaining != 0 {
            if byte < self.min || byte > self.max {
                return Err(());
            }
            self.remaining -= 1;
            self.min = 0x80;
            self.max = 0xbf;
            return Ok(());
        }
        match byte {
            0..=0x7f => {}
            0xc2..=0xdf => self.remaining = 1,
            0xe0 => {
                self.remaining = 2;
                self.min = 0xa0;
            }
            0xe1..=0xec | 0xee..=0xef => self.remaining = 2,
            0xed => {
                self.remaining = 2;
                self.max = 0x9f;
            }
            0xf0 => {
                self.remaining = 3;
                self.min = 0x90;
            }
            0xf1..=0xf3 => self.remaining = 3,
            0xf4 => {
                self.remaining = 3;
                self.max = 0x8f;
            }
            _ => return Err(()),
        }
        Ok(())
    }
}
