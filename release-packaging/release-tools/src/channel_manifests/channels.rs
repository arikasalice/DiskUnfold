//! The three channel documents.

use super::{
    ChannelContext, FORMAT_UNAVAILABLE, InventoryDocument, Unavailable, cpu_label, declared,
    file_entry, header, rows_for, store_signed, update_method,
};
use crate::release_inventory::{DistributionChannel, PackageFormat, UpdateChannel, UpdateMethod};
use serde_json::{Value, json};

fn unavailable(
    inventory: &InventoryDocument,
    context: &ChannelContext<'_>,
    channel: UpdateChannel,
    why: &Unavailable<'_>,
) -> Value {
    let mut document = header(inventory, context, why.channel, "unavailable");
    document["reason_key"] = json!(why.reason_key);
    document["reason"] = json!(why.reason);
    document["entries"] = json!([]);
    document["declared_references"] = json!(declared(inventory, channel));
    document
}

/// Direct download from the official release page.
pub(super) fn direct(
    inventory: &InventoryDocument,
    context: &ChannelContext<'_>,
) -> Result<Value, serde_json::Error> {
    let status = if context.tag.is_some() {
        "available"
    } else {
        "developer_build"
    };
    let mut document = header(inventory, context, "direct", status);
    let files = inventory
        .rows
        .iter()
        .filter(|row| row.channel == DistributionChannel::Direct)
        .map(|row| file_entry(row, context))
        .collect::<Result<Vec<_>, _>>()?;
    document["update_method"] = update_method(UpdateMethod::ReplaceFile);
    document["release_page"] = context.tag.map_or(Value::Null, |tag| {
        json!(format!(
            "https://github.com/{}/releases/tag/{tag}",
            context.repository
        ))
    });
    document["checksums_file"] = json!("SHA256SUMS");
    document["note"] = json!(if context.tag.is_some() {
        "Served from the release page once a person publishes the draft release."
    } else {
        "Developer build: nothing is published and no download address exists."
    });
    document["entries"] = json!(files);
    Ok(document)
}

/// winget: the designed installer is an MSI, and the publisher part of the package identifier is
/// still a design decision.
pub(super) fn winget(
    inventory: &InventoryDocument,
    context: &ChannelContext<'_>,
) -> Result<Value, serde_json::Error> {
    let why = |reason_key, reason| Unavailable {
        channel: "winget",
        reason_key,
        reason,
    };
    let rows = rows_for(inventory, PackageFormat::Msi, DistributionChannel::Winget);
    let blocked = if rows.is_empty() {
        Some(why(
            Some(FORMAT_UNAVAILABLE),
            "no MSI package is built yet, and winget installs the MSI",
        ))
    } else if context.winget_identifier.is_none() {
        Some(why(
            None,
            "the winget package identifier is an open design decision (publisher identifier)",
        ))
    } else if context.tag.is_none() {
        Some(why(
            None,
            "a developer build has no public installer address",
        ))
    } else {
        None
    };
    if let Some(blocked) = blocked {
        return Ok(unavailable(
            inventory,
            context,
            UpdateChannel::Winget,
            &blocked,
        ));
    }
    let mut document = header(inventory, context, "winget", "available");
    let identifier = context.winget_identifier.unwrap_or_default();
    let entries = rows
        .iter()
        .map(|row| {
            json!({
                "package_identifier": identifier,
                "version": row.version,
                "installer_url": super::url(context, &row.file_name),
                "installer_sha256": row.sha256,
                "installer_type": "msi",
                "architecture": cpu_label(row.cpu),
                "update_method": update_method(UpdateMethod::ChannelCommand),
            })
        })
        .collect::<Vec<_>>();
    document["entries"] = json!(entries);
    document["note"] = json!("The release manager checks and submits this manifest.");
    Ok(document)
}

/// Microsoft Store: needs a Store-signed MSIX with the Store publisher identity.
pub(super) fn store(
    inventory: &InventoryDocument,
    context: &ChannelContext<'_>,
) -> Result<Value, serde_json::Error> {
    let rows: Vec<_> = rows_for(inventory, PackageFormat::Msix, DistributionChannel::Store)
        .into_iter()
        .filter(|row| store_signed(row))
        .collect();
    if rows.is_empty() {
        let why = Unavailable {
            channel: "store",
            reason_key: Some(FORMAT_UNAVAILABLE),
            reason: "no Store-signed MSIX submission package exists; the MSIX is an unsigned development package and the Store publisher identity is not set",
        };
        return Ok(unavailable(inventory, context, UpdateChannel::Store, &why));
    }
    let mut document = header(inventory, context, "store", "available");
    let entries = rows
        .iter()
        .map(|row| file_entry(row, context))
        .collect::<Result<Vec<_>, _>>()?;
    document["entries"] = json!(entries);
    document["update_method"] = update_method(UpdateMethod::Automatic);
    document["listing"] =
        json!("not generated: the Store listing information is supplied by the release manager");
    document["note"] = json!("The release manager checks and submits this package.");
    Ok(document)
}
