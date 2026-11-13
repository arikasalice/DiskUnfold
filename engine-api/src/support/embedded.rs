//! The embedded support matrix as the product and startup entry points trust it.
use du_core_contracts::product_support::{EmbeddedProductInfo, SupportMatrix};

/// The embedded support matrix when it can be trusted.
///
/// Returns `None` when the build embedded no matrix, or when the revision the
/// build recorded differs from the revision the matrix carries. Callers treat
/// `None` as an empty matrix, which `evaluate` reports as unknown. This is the
/// single owner of that consistency rule for the product-info command, the
/// support-info command and the startup check.
pub fn embedded_matrix(info: &EmbeddedProductInfo) -> Option<&SupportMatrix> {
    let matrix = info.support_matrix.as_ref()?;
    match (&info.build.support_matrix_revision, &matrix.revision) {
        (Some(built), Some(embedded)) if built != embedded => None,
        _ => Some(matrix),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::product_support::{BuildKind, ProductBuildInfo, ProductNotices};

    fn info(built: Option<&str>, matrix: Option<SupportMatrix>) -> EmbeddedProductInfo {
        EmbeddedProductInfo {
            build: ProductBuildInfo {
                product_version: None,
                build_kind: BuildKind::Unknown,
                commit: None,
                built_at_utc: None,
                target_os: None,
                target_cpu: None,
                app_db_format: None,
                snapshot_format: None,
                support_matrix_revision: built.map(str::to_owned),
            },
            notices: ProductNotices {
                license: None,
                third_party: None,
                source_commit: None,
            },
            support_matrix: matrix,
        }
    }

    fn matrix(revision: Option<&str>) -> SupportMatrix {
        SupportMatrix {
            revision: revision.map(str::to_owned),
            ..SupportMatrix::default()
        }
    }

    #[test]
    fn an_absent_matrix_or_a_contradicting_build_revision_is_not_trusted() {
        assert!(embedded_matrix(&info(Some("m1"), None)).is_none());
        assert!(embedded_matrix(&info(Some("m0"), Some(matrix(Some("m1"))))).is_none());
    }

    #[test]
    fn an_agreeing_or_uncontradicted_revision_is_trusted() {
        for (built, embedded) in [
            (Some("m1"), Some("m1")),
            (None, Some("m1")),
            (Some("m1"), None),
            (None, None),
        ] {
            let candidate = info(built, Some(matrix(embedded)));
            assert_eq!(
                embedded_matrix(&candidate).map(|value| value.revision.as_deref()),
                Some(embedded),
                "{built:?} {embedded:?}"
            );
        }
    }
}
