//! Project location from the already admitted immutable page.
use crate::browse::field;
use du_analysis_index::{
    ResultVersion,
    query::{FolderLocation, page::Page},
};
use du_core_contracts::{
    ItemRef, PathEncoding,
    browse_page_ipc::{
        BrowseBreadcrumb, BrowseContext, BrowseRootKind, QueryStrategy, ResultOrigin,
    },
    result::{ResultItem, ResultScopeRoot, ResultVersionRead},
    volume::RootVolumeGroup,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in super::super) struct StartupVolumeRoot {
    pub item_no: u32,
    pub name: Option<String>,
}

pub(in super::super) fn startup_volume_root(version: &ResultVersion) -> Option<StartupVolumeRoot> {
    startup_root(
        version.scope_roots(),
        version.observed_roots(),
        version.volume_group(),
    )
}

fn startup_root(
    scope_roots: &[ResultItem],
    observed_roots: &[ResultScopeRoot],
    group: Option<&RootVolumeGroup>,
) -> Option<StartupVolumeRoot> {
    let [scope_root] = scope_roots else {
        return None;
    };
    let [observed_root] = observed_roots else {
        return None;
    };
    if observed_root.path.encoding != PathEncoding::Unix
        || observed_root.path.raw.bytes().ok().as_deref() != Some(b"/")
    {
        return None;
    }
    Some(StartupVolumeRoot {
        item_no: scope_root.item_no,
        name: group?.system_name.clone(),
    })
}

pub(super) fn page_context(page: &Page) -> Result<Option<BrowseContext>, super::super::Error> {
    let Some(location) = page.folder_location() else {
        return Ok(None);
    };
    let version = page.version();
    let requested = match location {
        FolderLocation::ScopeRoots => None,
        FolderLocation::Item(item_no) => Some(du_core_contracts::ItemRef {
            result_version: version.version_id().clone(),
            item_no,
        }),
    };
    let (location, breadcrumbs, _) = if requested.is_some() {
        super::super::open_location(version, requested.as_ref())?
    } else {
        (None, Vec::new(), 0)
    };
    let excluded_children =
        super::super::collection::excluded_children(version, location.as_ref())?;
    let metadata = page.metadata();
    project_context(
        ResultOrigin("scan".into()),
        version.data_source(),
        location,
        breadcrumbs,
        metadata.sorts,
        metadata.predicate_strategies,
        excluded_children,
    )
    .map(Some)
}

/// Projects location and strategy evidence consistently for open and page responses.
///
/// `excluded_children` must be read from the same pinned version as `location`.
pub(in super::super) fn project_context(
    origin: ResultOrigin,
    data_source: du_core_contracts::data_source::DataSource,
    location: Option<ItemRef>,
    breadcrumbs: Vec<super::super::Breadcrumb>,
    sorts: &[du_analysis_index::query::SortSpec],
    predicates: &[(String, u32)],
    excluded_children: Option<du_core_contracts::browse_page_ipc::QueryTotal>,
) -> Result<BrowseContext, super::super::Error> {
    let root_kind = match (location.is_some(), breadcrumbs.len()) {
        (false, 0) => BrowseRootKind::VirtualRoot,
        (true, 1) => BrowseRootKind::ScopeRoot,
        (true, 2..) => BrowseRootKind::Folder,
        _ => return Err(super::super::Error::Internal),
    };
    Ok(BrowseContext {
        origin,
        data_source: Some(data_source),
        location,
        breadcrumbs: breadcrumbs
            .into_iter()
            .map(|crumb| BrowseBreadcrumb {
                item: crumb.item,
                display_name: field(crumb.display_name, "ref.unavailable"),
                is_startup_volume: crumb.is_startup_volume.then_some(true),
            })
            .collect(),
        root_kind,
        current_correspondence: false,
        sort_strategies: sorts
            .iter()
            .map(|s| QueryStrategy {
                id: s.id.clone(),
                revision: s.revision,
            })
            .collect(),
        predicate_strategies: predicates
            .iter()
            .map(|(id, revision)| QueryStrategy {
                id: id.clone(),
                revision: *revision,
            })
            .collect(),
        excluded_children,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::{
        Id, PathBytes,
        data_source::DataSource,
        result::{ResultItemKind, ResultNativePath},
        volume::ConfirmedFirmlink,
    };

    fn root(path: &[u8], encoding: PathEncoding) -> ResultScopeRoot {
        ResultScopeRoot {
            path: ResultNativePath {
                encoding,
                raw: PathBytes::from_bytes(path),
            },
            volume_id: vec![1],
            file_id: vec![2],
        }
    }

    fn item(item_no: u32) -> ResultItem {
        ResultItem {
            item_no,
            parent: None,
            name: b"/".to_vec(),
            size: None,
            kind: ResultItemKind::Directory,
        }
    }

    fn group(name: Option<&str>) -> RootVolumeGroup {
        RootVolumeGroup {
            data_mount: b"/System/Volumes/Data".to_vec(),
            system_name: name.map(str::to_owned),
            device: 1,
            data_inode: 2,
            firmlinks: vec![ConfirmedFirmlink {
                source: b"/Users".to_vec(),
                target: b"/System/Volumes/Data/Users".to_vec(),
                device: 1,
                inode: 3,
            }],
        }
    }

    #[test]
    fn startup_root_requires_one_confirmed_unix_root_item() {
        let roots = [item(7)];
        let observed = [root(b"/", PathEncoding::Unix)];
        assert_eq!(
            startup_root(&roots, &observed, Some(&group(Some("Macintosh HD")))),
            Some(StartupVolumeRoot {
                item_no: 7,
                name: Some("Macintosh HD".into()),
            })
        );
        assert_eq!(
            startup_root(&roots, &observed, Some(&group(None))).map(|root| root.name),
            Some(None)
        );
        assert_eq!(startup_root(&roots, &observed, None), None);
        assert_eq!(
            startup_root(
                &roots,
                &[
                    root(b"/", PathEncoding::Unix),
                    root(b"/Data", PathEncoding::Unix)
                ],
                Some(&group(None))
            ),
            None
        );
        assert_eq!(
            startup_root(&[item(7), item(8)], &observed, Some(&group(None))),
            None
        );
        assert_eq!(
            startup_root(
                &roots,
                &[root(b"/Users", PathEncoding::Unix)],
                Some(&group(None))
            ),
            None
        );
        assert_eq!(
            startup_root(
                &roots,
                &[root(b"/", PathEncoding::WindowsUtf16le)],
                Some(&group(None))
            ),
            None
        );
    }

    #[test]
    fn projection_marks_only_the_startup_root_and_uses_its_reported_name()
    -> Result<(), Box<dyn std::error::Error>> {
        let id = Id::from_v7_bytes([7; 16]);
        let crumb = |item_no, display_name: Option<&str>, is_startup_volume| {
            super::super::super::Breadcrumb {
                item: ItemRef {
                    result_version: id.clone(),
                    item_no,
                },
                name: b"/".to_vec(),
                display_name: display_name.map(str::to_owned),
                is_startup_volume,
            }
        };
        let context = project_context(
            ResultOrigin("scan".into()),
            DataSource::Real,
            Some(ItemRef {
                result_version: id.clone(),
                item_no: 0,
            }),
            vec![
                crumb(0, Some("Macintosh HD"), true),
                crumb(1, Some("Data"), false),
            ],
            &[],
            &[],
            None,
        )
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        assert_eq!(context.breadcrumbs[0].is_startup_volume, Some(true));
        assert_eq!(
            context.breadcrumbs[0].display_name,
            du_core_contracts::browse_ipc::BrowseValue::Present {
                value: "Macintosh HD".into()
            }
        );
        assert_eq!(context.breadcrumbs[1].is_startup_volume, None);
        assert_eq!(
            context.breadcrumbs[1].display_name,
            du_core_contracts::browse_ipc::BrowseValue::Present {
                value: "Data".into()
            }
        );
        Ok(())
    }
}
