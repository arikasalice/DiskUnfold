//! Text and JSON renderers for the F15 product information commands.

use std::io::{self, Write};

use du_core_contracts::product_support::{
    BuildKind, EmbeddedProductInfo, ProductBuildInfo, ProductNotices, SecuritySupportKind,
    SupportCapabilityState, SupportCombination, SupportCpu, SupportMatrix, SupportOsFamily,
    SupportReport, SupportStatus,
};
use du_user_env::i18n::Registry;

use crate::args::NoticeDocument;

pub(crate) struct Messages<'a> {
    registry: &'a Registry,
    language: &'a str,
}

impl<'a> Messages<'a> {
    pub(crate) fn new(registry: &'a Registry, language: &'a str) -> Self {
        Self { registry, language }
    }

    pub(crate) fn resolve(&self, key: &str) -> Option<String> {
        self.registry.resolve_plain_text(self.language, key)
    }
}

pub(crate) fn default_product_info(version: &str) -> EmbeddedProductInfo {
    EmbeddedProductInfo {
        build: ProductBuildInfo {
            product_version: Some(version.to_owned()),
            build_kind: BuildKind::Developer,
            commit: None,
            built_at_utc: None,
            target_os: Some(std::env::consts::OS.to_owned()),
            target_cpu: Some(std::env::consts::ARCH.to_owned()),
            app_db_format: None,
            snapshot_format: None,
            support_matrix_revision: None,
        },
        notices: ProductNotices {
            license: Some(include_str!("../../LICENSE").to_owned()),
            third_party: None,
            source_commit: None,
        },
        support_matrix: None,
    }
}

pub(crate) fn notice_text(
    document: NoticeDocument,
    product_info: &EmbeddedProductInfo,
) -> Option<&str> {
    match document {
        NoticeDocument::License => product_info.notices.license.as_deref(),
        NoticeDocument::ThirdParty => product_info.notices.third_party.as_deref(),
    }
    // IF-02 treats a blank embedded document as not embedded.
    .filter(|text| !text.trim().is_empty())
}

/// Returns the embedded matrix unless the build records a different revision.
///
/// This is the same rule IF-04 applies, so `support` and the support screen
/// evaluate the same table.
pub(crate) fn consistent_matrix(product_info: &EmbeddedProductInfo) -> Option<&SupportMatrix> {
    let matrix = product_info.support_matrix.as_ref()?;
    match (
        &product_info.build.support_matrix_revision,
        &matrix.revision,
    ) {
        (Some(built), Some(embedded)) if built != embedded => None,
        _ => Some(matrix),
    }
}

/// Matrix revision shown by `version`, derived as IF-01 derives it.
fn support_matrix_revision(product_info: &EmbeddedProductInfo) -> Option<String> {
    consistent_matrix(product_info).and_then(|matrix| {
        product_info
            .build
            .support_matrix_revision
            .clone()
            .or_else(|| matrix.revision.clone())
    })
}

pub(crate) fn write_version(
    stdout: &mut impl Write,
    message: &Messages<'_>,
    product_info: &EmbeddedProductInfo,
) -> io::Result<()> {
    let unknown = message
        .resolve("cli.product.unknown")
        .unwrap_or_else(|| "Unknown".to_owned());
    let build_kind_key = match product_info.build.build_kind {
        BuildKind::Official => "cli.product.build_official",
        BuildKind::Developer => "cli.product.build_developer",
        BuildKind::Unknown => "cli.product.build_unknown",
    };
    let fields = [
        (
            "cli.product.version",
            product_info.build.product_version.clone(),
        ),
        ("cli.product.build_kind", message.resolve(build_kind_key)),
        ("cli.product.commit", product_info.build.commit.clone()),
        (
            "cli.product.built_at",
            product_info.build.built_at_utc.clone(),
        ),
        (
            "cli.product.target_os",
            product_info.build.target_os.clone(),
        ),
        (
            "cli.product.target_cpu",
            product_info.build.target_cpu.clone(),
        ),
        (
            "cli.product.app_db_format",
            product_info
                .build
                .app_db_format
                .map(|value| value.to_string()),
        ),
        (
            "cli.product.snapshot_format",
            product_info
                .build
                .snapshot_format
                .map(|value| value.to_string()),
        ),
        (
            "cli.product.support_matrix",
            support_matrix_revision(product_info),
        ),
    ];
    for (key, value) in fields {
        let label = message.resolve(key).unwrap_or_else(|| key.to_owned());
        writeln!(
            stdout,
            "{label}: {}",
            value.unwrap_or_else(|| unknown.clone())
        )?;
    }
    Ok(())
}

pub(crate) fn write_support_text(
    stdout: &mut impl Write,
    message: &Messages<'_>,
    report: &SupportReport,
) -> io::Result<()> {
    let label = |key: &str| message.resolve(key).unwrap_or_else(|| key.to_owned());
    let unknown = || {
        message
            .resolve("cli.product.unknown")
            .unwrap_or_else(|| "Unknown".to_owned())
    };

    writeln!(stdout, "{}", label("cli.support.title"))?;
    write_field(
        stdout,
        &label("cli.support.matrix_revision"),
        report
            .matrix_revision
            .as_deref()
            .map(str::to_owned)
            .unwrap_or_else(unknown),
    )?;
    write_field(
        stdout,
        &label("cli.support.as_of"),
        report
            .as_of
            .as_deref()
            .map(str::to_owned)
            .unwrap_or_else(unknown),
    )?;
    writeln!(stdout, "{}:", label("cli.support.environment"))?;
    let identity = &report.environment;
    write_field(
        stdout,
        &label("cli.support.os_family"),
        localized_os(message, identity.os_family),
    )?;
    write_field(
        stdout,
        &label("cli.support.release"),
        identity.release.clone().unwrap_or_else(unknown),
    )?;
    write_field(
        stdout,
        &label("cli.support.edition"),
        identity.edition.clone().unwrap_or_else(unknown),
    )?;
    write_field(
        stdout,
        &label("cli.support.cpu"),
        localized_cpu(message, identity.cpu),
    )?;
    write_field(
        stdout,
        &label("cli.support.combination_status"),
        localized_status(message, report.combination_status),
    )?;
    if let Some(basis) = &report.security_support {
        writeln!(stdout, "{}:", label("cli.support.security_support"))?;
        if let Some(kind) = basis.kind {
            write_field(
                stdout,
                &label("cli.support.security_kind"),
                localized_security_kind(message, kind),
            )?;
        }
        write_field(
            stdout,
            &label("cli.support.security_source"),
            basis.source.clone().unwrap_or_else(unknown),
        )?;
        write_field(
            stdout,
            &label("cli.support.security_checked_on"),
            basis.checked_on.clone().unwrap_or_else(unknown),
        )?;
    }

    writeln!(stdout, "{}:", label("cli.support.features"))?;
    if report.features.is_empty() {
        writeln!(stdout, "{}", label("cli.support.no_combinations"))?;
    }
    for feature in &report.features {
        writeln!(stdout, "  {}", localized_area(message, &feature.area))?;
        write_field(
            stdout,
            &format!("    {}", label("cli.support.table_status")),
            localized_status(message, feature.support_table_status),
        )?;
        write_field(
            stdout,
            &format!("    {}", label("cli.support.current_capability")),
            localized_capability(message, feature.current_capability),
        )?;
        if !feature.conditions.is_empty() {
            write_field(
                stdout,
                &format!("    {}", label("cli.support.conditions")),
                feature.conditions.join("; "),
            )?;
        }
        if let Some(reason) = &feature.reason_key {
            write_field(
                stdout,
                &format!("    {}", label("cli.support.reason")),
                localized_reason(message, reason),
            )?;
        }
    }

    writeln!(stdout, "{}:", label("cli.support.combinations"))?;
    if report.combinations.is_empty() {
        writeln!(stdout, "{}", label("cli.support.no_combinations"))?;
    }
    for row in &report.combinations {
        write_combination(stdout, message, row)?;
    }
    if let Some(reason) = &report.reason_key {
        write_field(
            stdout,
            &label("cli.support.reason"),
            localized_reason(message, reason),
        )?;
    }
    Ok(())
}

fn write_combination(
    stdout: &mut impl Write,
    message: &Messages<'_>,
    row: &SupportCombination,
) -> io::Result<()> {
    let unknown = || {
        message
            .resolve("cli.product.unknown")
            .unwrap_or_else(|| "Unknown".to_owned())
    };
    let release = row.identity.release.clone().unwrap_or_else(&unknown);
    let edition = row.identity.edition.clone().unwrap_or_else(&unknown);
    writeln!(
        stdout,
        "  {} / {release} / {edition} / {}: {}",
        localized_os(message, row.identity.os_family),
        localized_cpu(message, row.identity.cpu),
        localized_status(message, row.status)
    )?;
    if let Some(basis) = &row.security_support {
        if let Some(kind) = basis.kind {
            write_field(
                stdout,
                &format!(
                    "    {}",
                    message
                        .resolve("cli.support.security_kind")
                        .unwrap_or_default()
                ),
                localized_security_kind(message, kind),
            )?;
        }
        write_field(
            stdout,
            &format!(
                "    {}",
                message
                    .resolve("cli.support.security_source")
                    .unwrap_or_default()
            ),
            basis
                .source
                .as_deref()
                .map(str::to_owned)
                .unwrap_or_else(unknown),
        )?;
        write_field(
            stdout,
            &format!(
                "    {}",
                message
                    .resolve("cli.support.security_checked_on")
                    .unwrap_or_default()
            ),
            basis
                .checked_on
                .as_deref()
                .map(str::to_owned)
                .unwrap_or_else(unknown),
        )?;
    }
    for feature in &row.features {
        writeln!(
            stdout,
            "    {}: {}",
            localized_area(message, &feature.area),
            localized_status(message, feature.status)
        )?;
        for condition in &feature.conditions {
            write_field(
                stdout,
                &format!(
                    "      {}",
                    message
                        .resolve("cli.support.conditions")
                        .unwrap_or_default()
                ),
                condition,
            )?;
        }
    }
    if !row.verified_details.is_empty() {
        write_field(
            stdout,
            &format!(
                "    {}",
                message
                    .resolve("cli.support.verified_details")
                    .unwrap_or_default()
            ),
            row.verified_details.join("; "),
        )?;
    }
    if !row.verification_records.is_empty() {
        write_field(
            stdout,
            &format!(
                "    {}",
                message
                    .resolve("cli.support.verification_records")
                    .unwrap_or_default()
            ),
            row.verification_records.join(", "),
        )?;
    }
    Ok(())
}

fn write_field(stdout: &mut impl Write, label: &str, value: impl AsRef<str>) -> io::Result<()> {
    writeln!(stdout, "{label}: {}", value.as_ref())
}

fn localized_os(message: &Messages<'_>, value: SupportOsFamily) -> String {
    let key = match value {
        SupportOsFamily::Windows => "cli.support.os.windows",
        SupportOsFamily::Macos => "cli.support.os.macos",
        SupportOsFamily::Linux => "cli.support.os.linux",
        SupportOsFamily::Unknown => "cli.support.os.unknown",
    };
    message.resolve(key).unwrap_or_else(|| key.to_owned())
}

fn localized_area(message: &Messages<'_>, value: &str) -> String {
    let key = match value {
        "full_disk_access" => "cli.support.area.full_disk_access",
        "webview" => "cli.support.area.webview",
        _ => return value.to_owned(),
    };
    message.resolve(key).unwrap_or_else(|| value.to_owned())
}

fn localized_cpu(message: &Messages<'_>, value: SupportCpu) -> String {
    let key = match value {
        SupportCpu::X64 => "cli.support.cpu.x64",
        SupportCpu::Arm64 => "cli.support.cpu.arm64",
        SupportCpu::Other => "cli.support.cpu.other",
        SupportCpu::Unknown => "cli.support.cpu.unknown",
    };
    message.resolve(key).unwrap_or_else(|| key.to_owned())
}

fn localized_status(message: &Messages<'_>, value: SupportStatus) -> String {
    let key = match value {
        SupportStatus::OfficiallyVerified => "cli.support.status.officially_verified",
        SupportStatus::Conditional => "cli.support.status.conditional",
        SupportStatus::UnsupportedFound => "cli.support.status.unsupported_found",
        SupportStatus::NotVerified => "cli.support.status.not_verified",
        SupportStatus::Unknown => "cli.support.status.unknown",
    };
    message.resolve(key).unwrap_or_else(|| key.to_owned())
}

fn localized_capability(message: &Messages<'_>, value: SupportCapabilityState) -> String {
    let key = match value {
        SupportCapabilityState::Supported => "cli.support.capability.supported",
        SupportCapabilityState::Limited => "cli.support.capability.limited",
        SupportCapabilityState::Unsupported => "cli.support.capability.unsupported",
        SupportCapabilityState::NotApplicable => "cli.support.capability.not_applicable",
        SupportCapabilityState::Unknown => "cli.support.capability.unknown",
    };
    message.resolve(key).unwrap_or_else(|| key.to_owned())
}

fn localized_security_kind(message: &Messages<'_>, value: SecuritySupportKind) -> String {
    let key = match value {
        SecuritySupportKind::General => "cli.support.security_general",
        SecuritySupportKind::Extended => "cli.support.security_extended",
        SecuritySupportKind::Unknown => "cli.support.security_unknown",
    };
    message.resolve(key).unwrap_or_else(|| key.to_owned())
}

fn localized_reason(message: &Messages<'_>, reason: &str) -> String {
    let key = match reason {
        "support.env_unknown" => "cli.support.reason.env_unknown",
        "support.outside_verified" => "cli.support.reason.outside_verified",
        "support.matrix_unavailable" => "cli.support.reason.matrix_unavailable",
        "support.matrix_invalid" => "cli.support.reason.matrix_invalid",
        "support.capability_unknown" => "cli.support.reason.capability_unknown",
        "startup.full_disk_access_required" => "cli.support.reason.full_disk_access_required",
        _ => return reason.to_owned(),
    };
    message.resolve(key).unwrap_or_else(|| reason.to_owned())
}
