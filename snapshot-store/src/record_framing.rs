//! Incremental v1 record framing, without decoding any payload semantics.

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Start,
    Items,
    Gaps,
    End,
}

pub(crate) struct Validator {
    phase: Phase,
    header: [u8; 4],
    header_len: usize,
    remaining: u32,
    expecting_kind: bool,
    count: u64,
    expected: u64,
}

impl Validator {
    pub(crate) fn new(expected: u64) -> Self {
        Self {
            phase: Phase::Start,
            header: [0; 4],
            header_len: 0,
            remaining: 0,
            expecting_kind: false,
            count: 0,
            expected,
        }
    }

    pub(crate) fn push(&mut self, mut input: &[u8]) -> Result<(), ()> {
        while !input.is_empty() {
            if self.phase == Phase::End && self.remaining == 0 {
                return Err(());
            }
            if self.remaining == 0 {
                let take = (4 - self.header_len).min(input.len());
                self.header[self.header_len..self.header_len + take]
                    .copy_from_slice(&input[..take]);
                self.header_len += take;
                input = &input[take..];
                if self.header_len != 4 {
                    continue;
                }
                self.remaining = u32::from_le_bytes(self.header);
                self.header_len = 0;
                if !(1..=65_536).contains(&self.remaining) {
                    return Err(());
                }
                self.expecting_kind = true;
            }
            if self.expecting_kind && !input.is_empty() {
                self.kind(input[0])?;
                self.expecting_kind = false;
                self.remaining -= 1;
                input = &input[1..];
            }
            if !self.expecting_kind {
                let consumed = (self.remaining as usize).min(input.len());
                input = &input[consumed..];
                self.remaining -= consumed as u32;
                if self.remaining == 0 {
                    self.count = self.count.checked_add(1).ok_or(())?;
                    if self.count > self.expected {
                        return Err(());
                    }
                }
            }
        }
        Ok(())
    }

    fn kind(&mut self, kind: u8) -> Result<(), ()> {
        self.phase = match (self.phase, kind) {
            (Phase::Start, 1) => Phase::Items,
            (Phase::Items, 2) => Phase::Items,
            (Phase::Items | Phase::Gaps, 3) => Phase::Gaps,
            (Phase::Items | Phase::Gaps, 0x7f) if self.remaining == 1 => Phase::End,
            _ => return Err(()),
        };
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<(), ()> {
        if self.phase == Phase::End
            && self.header_len == 0
            && self.remaining == 0
            && self.count == self.expected
        {
            Ok(())
        } else {
            Err(())
        }
    }
}

#[cfg(test)]
mod tests;
