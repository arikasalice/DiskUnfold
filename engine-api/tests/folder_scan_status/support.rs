//! Deterministic builders and wire readers shared by the scan status tests.
#![allow(dead_code)]
use du_analysis_index::{IngestSink, query};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding,
    browse_page_ipc::ResultOrigin,
    result::{ResultNativePath, ResultScopeRoot},
};
use du_engine_api::browse::folder::{
    FolderBrowse, LiveOpen, OpenRequest, Sort, View, cursor::Session,
};
use du_job_runtime::JobIdentity;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

pub type Check = Result<(), String>;

pub fn err<T: std::fmt::Debug>(value: T) -> String {
    format!("{value:?}")
}

pub fn present(value: impl Into<Value>) -> Value {
    json!({"state": "present", "value": value.into()})
}

pub fn absent() -> Value {
    json!({"state": "absent"})
}

pub fn unknown(reason_key: &str) -> Value {
    json!({"state": "unknown", "reasonKey": reason_key})
}

pub fn id(n: u8) -> Id {
    Id::from_v7_bytes([n; 16])
}

struct Ids(AtomicU8);

impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        Id::from_v7_bytes([1, 2, 3, 4, 5, 6, 0x70, n, 0x80, 0, 0, 0, 0, 0, 0, n])
    }
}

/// A GUI browse session with deterministic query identifiers and cursor key.
pub fn session(sink: IngestSink) -> Session {
    let browse = Arc::new(FolderBrowse::new(sink, Arc::new(Ids(AtomicU8::new(100)))));
    Session::new(browse, [7; 32])
}

pub fn root(path: &[u8]) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(path),
        },
        volume_id: vec![1],
        file_id: path.to_vec(),
    }
}

pub fn parts(names: &[&str]) -> Vec<Vec<u8>> {
    names.iter().map(|name| name.as_bytes().to_vec()).collect()
}

pub fn request(version: Option<Id>, location: Option<ItemRef>, page_size: u32) -> OpenRequest {
    OpenRequest {
        result_version: version,
        location,
        view: View::List,
        condition: query::Condition::All(vec![]),
        sort: Sort {
            id: "sort.path".into(),
            revision: 1,
            basis: query::Basis::Logical,
            descending: false,
        },
        page_size: Some(page_size),
    }
}

/// The open response, the first positional page and every continuation page of one open.
pub struct Wire {
    pub open: Value,
    pub first: Value,
    pub continued: Vec<Value>,
}

impl Wire {
    /// Every row of the folder in page order, from the open and continuation responses.
    pub fn rows(&self) -> Vec<&Value> {
        let mut rows = array(&self.open["page"]["rows"]);
        for page in &self.continued {
            rows.extend(array(&page["rows"]));
        }
        rows
    }

    /// The row whose display name is `name`.
    pub fn row(&self, name: &str) -> Result<&Value, String> {
        self.rows()
            .into_iter()
            .find(|row| row["displayName"]["value"] == name)
            .ok_or_else(|| format!("no row {name}"))
    }

    /// The row of `item`.
    pub fn item_row(&self, item: &ItemRef) -> Result<&Value, String> {
        self.rows()
            .into_iter()
            .find(|row| row["item"]["itemNo"] == item.item_no)
            .ok_or_else(|| format!("no row {}", item.item_no))
    }

    /// The excluded entries of the opened folder, as the open response carries them.
    pub fn excluded(&self) -> Option<&Value> {
        self.open.get("excludedChildren")
    }

    /// Every response of the open as one text.
    pub fn text(&self) -> String {
        let mut text = format!("{}{}", self.open, self.first);
        for page in &self.continued {
            text.push_str(&page.to_string());
        }
        text
    }

    /// Checks the producer invariants of a scan-lineage open on every path.
    pub fn check(&self) -> Check {
        // The open and the first position page come from the same pinned query.
        assert_eq!(
            self.excluded(),
            self.first["browseContext"].get("excludedChildren"),
            "{}",
            self.text()
        );
        assert_eq!(self.open["page"]["rows"], self.first["rows"]);
        for page in &self.continued {
            assert!(page.get("browseContext").is_none(), "{page}");
        }
        for row in self.rows() {
            check_row(row)?;
        }
        // Unread causes travel in the scan issue list, never on a row.
        for key in ["scan.cancelled", "scan.disconnected"] {
            assert!(!self.text().contains(key), "{key}");
        }
        Ok(())
    }
}

/// The producer invariants of one row of a scan-lineage version.
fn check_row(row: &Value) -> Check {
    let flags = &row["flags"];
    assert_eq!(flags["excluded"], present(false), "{row}");
    if row["kind"] != "directory" {
        assert!(flags.get("boundary").is_none(), "{row}");
        assert_eq!(flags["unenumerated"], absent(), "{row}");
        return Ok(());
    }
    let boundary = flags
        .get("boundary")
        .ok_or_else(|| format!("folder row without boundary: {row}"))?;
    if flags["unenumerated"] == present(true) {
        assert_ne!(boundary["state"], "present", "{row}");
    }
    for value in [&flags["unenumerated"], boundary] {
        if value["state"] == "unknown" {
            let reason = value["reasonKey"].as_str().unwrap_or_default();
            assert!(
                ["scan.in_progress", "space.data_insufficient"].contains(&reason),
                "{row}"
            );
        }
    }
    Ok(())
}

fn array(value: &Value) -> Vec<&Value> {
    value
        .as_array()
        .map(|rows| rows.iter().collect())
        .unwrap_or_default()
}

/// Opens one folder through the session and reads all of its pages on the wire.
pub fn browse(
    session: &Session,
    version: Option<Id>,
    location: Option<ItemRef>,
    page_size: u32,
) -> Result<Wire, String> {
    let opened = session
        .open(request(version, location, page_size), &[], || false)
        .map_err(err)?;
    let query = opened.opened.page.query_ref.clone();
    let pinned = opened.opened.page.result_version.clone();
    let mut cursor = opened.next_cursor.clone();
    let open = LiveOpen {
        request_seq: 7,
        page: opened,
    }
    .into_response(ResultOrigin("scan".into()))
    .map_err(err)?;
    let first = session
        .page_at(&query, &pinned, 0, Some(page_size))
        .map_err(err)?
        .into_response()
        .map_err(err)?;
    let mut continued = Vec::new();
    while let Some(next) = cursor.take() {
        let page = session.page(&next, Some(page_size)).map_err(err)?;
        cursor = page.next_cursor.clone();
        continued.push(serde_json::to_value(page.into_response().map_err(err)?).map_err(err)?);
    }
    Ok(Wire {
        open: serde_json::to_value(open).map_err(err)?,
        first: serde_json::to_value(first).map_err(err)?,
        continued,
    })
}

/// The reference of the item at `names` below the single root of `version`.
pub fn item(version: &du_analysis_index::ResultVersion, names: &[&str]) -> Result<ItemRef, String> {
    use du_core_contracts::result::ResultVersionRead;
    let mut current = version
        .items()
        .iter()
        .find(|item| item.parent.is_none())
        .ok_or("no root")?
        .item_no;
    for name in names {
        current = version
            .items()
            .iter()
            .find(|item| item.parent == Some(current) && item.name == name.as_bytes())
            .ok_or_else(|| format!("no item {name}"))?
            .item_no;
    }
    Ok(ItemRef {
        result_version: version.version_id().clone(),
        item_no: current,
    })
}
