//! Adapt the shared identity-bound content port to duplicate verification.
use crate::{
    candidates::FileIdentity,
    verification::{OpenFile, ReadError, ReadPort, Snapshot},
};
use du_core_contracts::{
    Id, ItemRef,
    duplicate_content::{BoundContent, ContentIdentity, DuplicateContentPort, PlaceholderEvidence},
    ports::PortErrorKind,
};
use std::collections::BTreeMap;

/// The identity and placeholder observation for one selected result item.
pub struct SelectedContent {
    pub item: ItemRef,
    pub identity: ContentIdentity,
    pub placeholder: PlaceholderEvidence,
}

/// Refuses mixed versions, duplicate items, or incomplete identity evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidSelection;

/// A bridge for one immutable selected set and one injected platform port.
pub struct Adapter<'a, P> {
    port: Port<'a, P>,
    selected: BTreeMap<u32, SelectedContent>,
}

enum Port<'a, P> {
    Borrowed(&'a mut P),
    Owned(P),
}

impl<'a, P> Adapter<'a, P> {
    /// Binds the exact selected item evidence before verification starts.
    ///
    /// # Errors
    /// Returns `InvalidSelection` for a mixed version, duplicate item, or empty identity.
    pub fn new(
        port: &'a mut P,
        version: &Id,
        selected: Vec<SelectedContent>,
    ) -> Result<Self, InvalidSelection> {
        Self::bind(Port::Borrowed(port), version, selected)
    }

    fn bind(
        port: Port<'a, P>,
        version: &Id,
        selected: Vec<SelectedContent>,
    ) -> Result<Self, InvalidSelection> {
        let mut items = BTreeMap::new();
        for item in selected {
            if &item.item.result_version != version
                || item.identity.volume.is_empty()
                || item.identity.file.is_empty()
                || item.identity.enumerated_inode == Some(0)
            {
                return Err(InvalidSelection);
            }
            if items.insert(item.item.item_no, item).is_some() {
                return Err(InvalidSelection);
            }
        }
        Ok(Self {
            port,
            selected: items,
        })
    }
}

impl<P> Adapter<'static, P> {
    /// Binds a port owned by the verification worker to one pinned selection.
    ///
    /// This lets a queued job retain the port without borrowing from its caller.
    /// The caller must keep the source result pinned for the duration of the job.
    ///
    /// # Errors
    /// Returns `InvalidSelection` for a mixed version, duplicate item, or empty identity.
    pub fn new_owned(
        port: P,
        version: &Id,
        selected: Vec<SelectedContent>,
    ) -> Result<Self, InvalidSelection> {
        Self::bind(Port::Owned(port), version, selected)
    }
}

impl<P: DuplicateContentPort> ReadPort for Adapter<'_, P> {
    fn open_bound<'a>(
        &'a mut self,
        item_no: u32,
        expected: &FileIdentity,
    ) -> Result<Box<dyn OpenFile + 'a>, ReadError> {
        let item = self.selected.get(&item_no).ok_or(ReadError::Failed)?;
        if item.identity.volume != expected.volume || item.identity.file != expected.file {
            return Err(ReadError::IdentityChanged);
        }
        if item.placeholder != PlaceholderEvidence::Local {
            return Err(ReadError::Failed);
        }
        let expected = item.identity.clone();
        let port = match &mut self.port {
            Port::Borrowed(port) => &mut **port,
            Port::Owned(port) => port,
        };
        let handle = port
            .open_bound(&item.item, &expected, item.placeholder)
            .map_err(map_error)?;
        Ok(Box::new(Opened { handle, expected }))
    }
}

struct Opened<'a> {
    handle: Box<dyn BoundContent + 'a>,
    expected: ContentIdentity,
}
impl OpenFile for Opened<'_> {
    fn snapshot(&mut self) -> Result<Snapshot, ReadError> {
        let value = self.handle.observe().map_err(map_error)?;
        if value.identity != self.expected {
            return Err(ReadError::IdentityChanged);
        }
        Ok(Snapshot {
            identity: FileIdentity {
                volume: value.identity.volume,
                file: value.identity.file,
            },
            size: value.size,
            modified: value.modified,
            change_stamp: value.change_stamp,
        })
    }
    fn read_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<usize, ReadError> {
        self.handle.read_at(offset, buffer).map_err(map_error)
    }
}

fn map_error(error: du_core_contracts::ports::PortError) -> ReadError {
    if error.kind == PortErrorKind::IdentityChanged {
        ReadError::IdentityChanged
    } else {
        ReadError::Failed
    }
}
