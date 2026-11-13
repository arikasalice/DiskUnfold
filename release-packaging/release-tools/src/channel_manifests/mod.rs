//! Per-channel release manifests (F15 FR1.HD1; architecture HLD 2.3.5 build and deployment
//! flow, IF-18).
//!
//! The manifests are generated from the release inventory, so they describe the same rows,
//! digests and SBOM names as `DISTRIBUTION.md` and `SHA256SUMS`. A channel is described as
//! `available` only when the packages it needs exist now. Otherwise its file says
//! `unavailable` and why, and lists no entry. The release manager submits to Store and winget;
//! this module only writes the files.

use crate::release_inventory::{
    ChannelReference, Cpu, DistributionChannel, DistributionRow, PackageFormat, SignatureMethod,
    UpdateChannel, UpdateMethod,
};
use serde::Deserialize;
use serde_json::{Value, json};

mod channels;
#[cfg(test)]
mod tests;

/// Reason key (warning): the channel needs a package format the pipeline cannot build yet.
pub const FORMAT_UNAVAILABLE: &str = crate::release_gate::FORMAT_UNAVAILABLE;

/// The parts of `release-inventory.json` the manifests are built from.
#[derive(Clone, Debug, Deserialize)]
pub struct InventoryDocument {
    /// Product name.
    pub product_name: String,
    /// Release version.
    pub version: String,
    /// Distribution rows in canonical order.
    pub rows: Vec<DistributionRow>,
    /// Store and winget references that point at rows.
    #[serde(default)]
    pub channel_references: Vec<ChannelReference>,
}

/// Where and how the release is published.
#[derive(Clone, Copy, Debug)]
pub struct ChannelContext<'a> {
    /// `owner/repo` that hosts the release page.
    pub repository: &'a str,
    /// The release tag. `None` for a developer build, which is never published.
    pub tag: Option<&'a str>,
    /// winget package identifier. The published design leaves the publisher part open, so it is
    /// supplied from outside once it is decided.
    pub winget_identifier: Option<&'a str>,
}

/// One generated manifest file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelFile {
    /// File name, for example `channel-direct.json`.
    pub file_name: &'static str,
    /// JSON text with a trailing newline.
    pub contents: String,
}

pub(crate) fn cpu_label(cpu: Cpu) -> &'static str {
    match cpu {
        Cpu::X64 => "x64",
        Cpu::Arm64 => "arm64",
        Cpu::Universal => "universal",
    }
}

pub(crate) fn update_method(method: UpdateMethod) -> Value {
    serde_json::to_value(method).unwrap_or(Value::Null)
}

fn url(context: &ChannelContext<'_>, file_name: &str) -> Value {
    context.tag.map_or(Value::Null, |tag| {
        json!(format!(
            "https://github.com/{}/releases/download/{tag}/{file_name}",
            context.repository
        ))
    })
}

pub(crate) fn file_entry(
    row: &DistributionRow,
    context: &ChannelContext<'_>,
) -> Result<Value, serde_json::Error> {
    Ok(json!({
        "file_name": row.file_name,
        "url": url(context, &row.file_name),
        "sha256": row.sha256,
        "os": serde_json::to_value(&row.os)?,
        "cpu": cpu_label(row.cpu),
        "format": serde_json::to_value(row.format)?,
        "signature": serde_json::to_value(row.signature)?,
        "signature_scope": row.signature_scope,
        "executable_file_name": row.executable_file_name,
        "executable_sha256": row.executable_sha256,
        "sbom_file_name": row.sbom_file_name,
        "run_instructions": row.run_instructions,
    }))
}

pub(crate) struct Unavailable<'a> {
    pub channel: &'static str,
    pub reason_key: Option<&'static str>,
    pub reason: &'a str,
}

pub(crate) fn header(
    inventory: &InventoryDocument,
    context: &ChannelContext<'_>,
    channel: &str,
    status: &str,
) -> Value {
    json!({
        "schema_version": 1,
        "channel": channel,
        "status": status,
        "product_name": inventory.product_name,
        "version": inventory.version,
        "build_kind": if context.tag.is_some() { "official" } else { "developer" },
        "tag": context.tag,
    })
}

pub(crate) fn declared(
    inventory: &InventoryDocument,
    channel: UpdateChannel,
) -> Vec<&ChannelReference> {
    inventory
        .channel_references
        .iter()
        .filter(|reference| reference.channel == channel)
        .collect()
}

pub(crate) fn rows_for(
    inventory: &InventoryDocument,
    format: PackageFormat,
    channel: DistributionChannel,
) -> Vec<&DistributionRow> {
    inventory
        .rows
        .iter()
        .filter(|row| {
            row.format == format
                && (row.channel == channel || row.channel == DistributionChannel::Direct)
        })
        .collect()
}

pub(crate) fn store_signed(row: &DistributionRow) -> bool {
    row.signature == SignatureMethod::Store
}

/// Builds the direct download, winget and Store manifests.
///
/// # Errors
/// Returns the serialization error when a manifest cannot be written.
pub fn build_channel_manifests(
    inventory: &InventoryDocument,
    context: &ChannelContext<'_>,
) -> Result<Vec<ChannelFile>, serde_json::Error> {
    let documents = [
        ("channel-direct.json", channels::direct(inventory, context)?),
        ("channel-winget.json", channels::winget(inventory, context)?),
        ("channel-store.json", channels::store(inventory, context)?),
    ];
    documents
        .into_iter()
        .map(|(file_name, document)| {
            Ok(ChannelFile {
                file_name,
                contents: serde_json::to_string_pretty(&document)? + "\n",
            })
        })
        .collect()
}
