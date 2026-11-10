//! Bounded Item and optional Gap checks after each chunk passes framing.
use crate::records::{NameEncoding, gap, item, volume};
use std::collections::HashSet;

pub(super) struct Validator {
    encoding: NameEncoding,
    root_count: Option<usize>,
    header: [u8; 4],
    header_len: usize,
    remaining: usize,
    kind_pending: bool,
    item_body: Option<Vec<u8>>,
    kind: u8,
    last_item: u32,
    volume_required: bool,
    pending_roots: HashSet<u32>,
}

impl Validator {
    pub(super) fn new(encoding: NameEncoding, root_count: Option<usize>) -> Self {
        Self {
            encoding,
            root_count,
            header: [0; 4],
            header_len: 0,
            remaining: 0,
            kind_pending: false,
            item_body: None,
            kind: 0,
            last_item: 0,
            volume_required: false,
            pending_roots: HashSet::new(),
        }
    }

    pub(super) fn require_volume(&mut self) {
        self.volume_required = true;
    }

    pub(super) fn finish(&self) -> Result<(), ()> {
        if self.volume_required || !self.pending_roots.is_empty() {
            Err(())
        } else {
            Ok(())
        }
    }

    /// The separate frame validator must receive the same chunk first.
    pub(super) fn push(&mut self, mut input: &[u8]) -> Result<(), ()> {
        while !input.is_empty() {
            if self.remaining == 0 {
                let n = (4 - self.header_len).min(input.len());
                self.header[self.header_len..self.header_len + n].copy_from_slice(&input[..n]);
                self.header_len += n;
                input = &input[n..];
                if self.header_len != 4 {
                    continue;
                }
                self.remaining = u32::from_le_bytes(self.header) as usize;
                self.header_len = 0;
                if !(1..=65_536).contains(&self.remaining) {
                    return Err(());
                }
                self.kind_pending = true;
            }
            if self.kind_pending && !input.is_empty() {
                let kind = input[0];
                self.kind = kind;
                input = &input[1..];
                self.remaining -= 1;
                self.kind_pending = false;
                self.item_body = (kind == 2
                    || (kind == 3 && self.root_count.is_some())
                    || (kind == 1 && self.volume_required))
                    .then(Vec::new);
                if self.remaining == 0 {
                    self.finish_item()?;
                }
            }
            if self.remaining > 0 && !self.kind_pending {
                let n = self.remaining.min(input.len());
                if let Some(body) = &mut self.item_body {
                    body.extend_from_slice(&input[..n]);
                }
                input = &input[n..];
                self.remaining -= n;
                if self.remaining == 0 {
                    self.finish_item()?;
                }
            }
        }
        Ok(())
    }

    fn finish_item(&mut self) -> Result<(), ()> {
        if let Some(body) = self.item_body.take() {
            if self.kind == 1 {
                let volume = volume::decode(&body, self.root_count.ok_or(())?).map_err(|_| ())?;
                self.pending_roots = volume.roots.iter().filter_map(|r| r.item_number).collect();
                self.volume_required = false;
            } else if self.kind == 2 {
                let item = item::decode(&body, self.encoding).map_err(|_| ())?;
                if self.last_item.checked_add(1) != Some(item.number) {
                    return Err(());
                }
                self.last_item = item.number;
                if self.pending_roots.remove(&item.number) {
                    let placeholder: &[u8] = match self.encoding {
                        NameEncoding::Bytes => b"@",
                        NameEncoding::WindowsUtf16Le => &[b'@', 0],
                    };
                    if item.parent != 0
                        || item.kind != item::Kind::Folder
                        || item.name != placeholder
                    {
                        return Err(());
                    }
                }
            } else if let Some(count) = self.root_count {
                gap::decode(&body, self.encoding, count).map_err(|_| ())?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::item::{Item, Kind, Size};

    #[test]
    fn every_chunk_boundary_preserves_item_checks() -> Result<(), String> {
        let body = Item {
            number: 1,
            parent: 0,
            kind: Kind::File,
            flags: 0,
            name: b"a",
            sizes: [Size::Unknown; 3],
            modified_ms: None,
            identity: &[],
            link_count: 0,
        }
        .encode(NameEncoding::Bytes)
        .map_err(|e| format!("{e:?}"))?;
        let mut data = vec![1, 0, 0, 0, 1];
        data.extend_from_slice(&((body.len() + 1) as u32).to_le_bytes());
        data.push(2);
        data.extend_from_slice(&body);
        data.extend_from_slice(&[1, 0, 0, 0, 127]);
        for split in 0..=data.len() {
            let mut framing = crate::record_framing::Validator::new(3);
            let mut items = Validator::new(NameEncoding::Bytes, None);
            for chunk in [&data[..split], &data[split..]] {
                framing.push(chunk).map_err(|()| "framing")?;
                items.push(chunk).map_err(|()| "item")?;
            }
            framing.finish().map_err(|()| "finish")?;
        }
        Ok(())
    }

    #[test]
    fn every_chunk_boundary_preserves_gap_checks_without_items() -> Result<(), String> {
        let body = gap::Gap {
            root_index: 1,
            path: vec![b"missing"],
            reason_key: "scan.denied",
            observed_at_ms: None,
        }
        .encode(NameEncoding::Bytes, 2)
        .map_err(|e| format!("{e:?}"))?;
        let mut data = vec![1, 0, 0, 0, 1];
        data.extend_from_slice(&((body.len() + 1) as u32).to_le_bytes());
        data.push(3);
        data.extend_from_slice(&body);
        data.extend_from_slice(&[1, 0, 0, 0, 127]);
        for split in 0..=data.len() {
            let mut framing = crate::record_framing::Validator::new(3);
            let mut items = Validator::new(NameEncoding::Bytes, Some(2));
            for chunk in [&data[..split], &data[split..]] {
                framing.push(chunk).map_err(|()| "framing")?;
                items.push(chunk).map_err(|()| "gap")?;
            }
            framing.finish().map_err(|()| "finish")?;
        }
        Ok(())
    }
}
