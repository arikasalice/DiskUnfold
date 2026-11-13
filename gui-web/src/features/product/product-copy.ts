import type { LocaleText } from "../../i18n/presentation";
import type { BuildKind, InstallForm, NoticeKind } from "../../ipc";

/** User-facing words for the product information screen (F15 IF-01 to IF-03). */
export function productCopy(text: LocaleText) {
  return {
    intro: text(
      "ui.product.intro",
      "These details come from the DiskUnfold program you are running. DiskUnfold reads them from itself and does not contact any server.",
    ),
    loading: text("ui.product.loading", "Loading product information"),
    loadFailed: text(
      "ui.product.load_failed",
      "Product information could not be read.",
    ),
    refreshFailed: text(
      "ui.product.refresh_failed",
      "Product information could not be refreshed. The values below are from the last successful read.",
    ),
    retry: text("ui.common.retry", "Retry"),
    refresh: text("ui.product.refresh", "Refresh"),
    buildHeading: text("ui.product.build_heading", "This build"),
    version: text("ui.product.version", "Version"),
    buildKind: text("ui.product.build_kind", "Build type"),
    commit: text("ui.product.commit", "Source commit"),
    builtAt: text("ui.product.built_at", "Built at (UTC)"),
    target: text("ui.product.target", "Built for"),
    installForm: text("ui.product.install_form", "Install form"),
    matrixRevision: text(
      "ui.product.matrix_revision",
      "Support table revision",
    ),
    matrixAsOf: text("ui.product.matrix_as_of", "Support table prepared on"),
    appDbFormat: text("ui.product.app_db_format", "App database format"),
    snapshotFormat: text("ui.product.snapshot_format", "Snapshot file format"),
    missing: text("ui.product.value_missing", "Not recorded in this build"),
    noticesHeading: text("ui.product.notices_heading", "Licenses and notices"),
    noticeMissing: text(
      "ui.product.notice_missing",
      "Not included in this build.",
    ),
    noticeLoading: text("ui.product.notice_loading", "Loading the document"),
    noticeFailed: text(
      "ui.product.notice_failed",
      "The document could not be read.",
    ),
    noticeEmpty: text(
      "ui.product.notice_empty",
      "This build does not include the text of this document.",
    ),
    noticeRegion: text("ui.product.notice_region", "Document text"),
    noticeCommit: text(
      "ui.product.notice_commit",
      "Generated from source commit",
    ),
    noticeCommitUnknown: text(
      "ui.product.notice_commit_unknown",
      "The source commit of this document is not recorded.",
    ),
    digestHeading: text("ui.product.digest_heading", "Executable file digest"),
    digestIntro: text(
      "ui.product.digest_intro",
      "Calculate the SHA-256 digest of the program file that is running now, then compare it with the checksum published on the official release page.",
    ),
    digestAction: text("ui.product.digest_action", "Calculate digest"),
    digestComputing: text(
      "ui.product.digest_computing",
      "Calculating the digest…",
    ),
    digestLabel: text("ui.product.digest_label", "SHA-256 digest"),
    digestCopy: text("ui.product.digest_copy", "Copy digest"),
    digestCopied: text("ui.product.digest_copied", "Digest copied."),
    digestCopyFailed: text(
      "ui.product.digest_copy_failed",
      "The digest could not be copied. Select it and copy it by hand.",
    ),
    digestCompare: text(
      "ui.product.digest_compare",
      "DiskUnfold does not treat this value as verified. This screen cannot tell whether the file is genuine; only your comparison with the official checksum can.",
    ),
    digestUnavailable: text(
      "ui.product.digest_unavailable",
      "DiskUnfold could not read its own executable file, so no digest is available. You can calculate the checksum of the file with your system tools.",
    ),
    digestUnavailableOther: text(
      "ui.product.digest_unavailable_other",
      "No digest is available. DiskUnfold reported a reason that this version does not recognize.",
    ),
    digestFailed: text(
      "ui.product.digest_failed",
      "The digest could not be requested. Try again.",
    ),
    installUnknown: text(
      "ui.product.install_unknown",
      "Could not be determined",
    ),
    installUnknownReason: text(
      "ui.product.install_unknown_reason",
      "DiskUnfold could not tell how it was installed. This does not change how it works.",
    ),
    installOtherReason: text(
      "ui.product.install_other_reason",
      "DiskUnfold reported a reason that this version does not recognize.",
    ),
    buildUnknownNote: text(
      "ui.product.build_unknown_note",
      "DiskUnfold could not tell whether this is an official or a developer build.",
    ),
    buildDeveloperNote: text(
      "ui.product.build_developer_note",
      "This is a developer build, not an official release.",
    ),
    buildOfficial: text("ui.product.build_official", "Official build"),
    buildDeveloper: text("ui.product.build_developer", "Developer build"),
    buildUnknown: text("ui.product.build_unknown", "Could not be determined"),
    installForms: {
      msi: text("ui.product.install_msi", "Windows installer (MSI)"),
      portable: text("ui.product.install_portable", "Portable, no installer"),
      msix: text("ui.product.install_msix", "Microsoft Store package (MSIX)"),
      dmg: text("ui.product.install_dmg", "macOS disk image (DMG)"),
      deb: text("ui.product.install_deb", "Debian package (DEB)"),
      tar_gz: text("ui.product.install_tar_gz", "Archive (tar.gz)"),
    } as Record<string, string>,
    noticeNames: {
      LICENSE: text("ui.product.notice_license", "License"),
      "THIRD-PARTY-NOTICES": text(
        "ui.product.notice_third_party",
        "Third-party notices",
      ),
    } as Record<NoticeKind, string>,
  };
}

export type ProductCopy = ReturnType<typeof productCopy>;

/** Chooses the words for a build type; an unfamiliar value reads as undetermined. */
export function buildKindText(kind: BuildKind, copy: ProductCopy) {
  if (kind === "official") return { value: copy.buildOfficial };
  if (kind === "developer") {
    return { value: copy.buildDeveloper, note: copy.buildDeveloperNote };
  }
  return { value: copy.buildUnknown, note: copy.buildUnknownNote };
}

/** Chooses the words for an install form; unknown is reported with its reason. */
export function installFormText(
  form: InstallForm,
  reasonKey: string | null,
  copy: ProductCopy,
) {
  const known = Object.hasOwn(copy.installForms, form)
    ? copy.installForms[form]
    : undefined;
  if (known) return { value: known };
  const note =
    reasonKey === null || reasonKey === "product.install_form_unknown"
      ? copy.installUnknownReason
      : copy.installOtherReason;
  return { value: copy.installUnknown, note };
}
