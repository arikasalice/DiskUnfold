# F15 macOS GUI defect fixes (branch codex/f15-mac-defects)

Evidence for the fixes is in `docs/handoff/2026-11-13/mac-gui-fix/` and `titlebar/` of the main checkout.

## Stable code-signing identity: how to confirm Full Disk Access survives an update

The DMG app is signed with the fixed self-signed identity "DiskUnfold Development Signing"
(repository secrets `MACOS_SELF_SIGNED_P12_BASE64` and `MACOS_SELF_SIGNED_P12_PASSWORD`).
Its designated requirement is `identifier "com.diskunfold.app" and certificate root = H"c688c680449b38e973409123eadfbf4260d8b855"`
for every build. The first install after the switch needs one new grant, because earlier builds
were ad-hoc signed (a `cdhash` requirement) and macOS no longer matches that entry.

1. Install a build signed with the identity into `/Applications`, open it, and grant Full Disk Access.
2. `sqlite3 "/Library/Application Support/com.apple.TCC/TCC.db" "select client,auth_value,hex(csreq) from access where service='kTCCServiceSystemPolicyAllFiles' and client='com.diskunfold.app'"` shows `auth_value` 2.
3. Quit the app, replace `/Applications/DiskUnfold.app` with a later build signed with the same identity, open it.
4. The startup screen reaches the main screen without a click (access confirmed), and the query above still shows `auth_value` 2.

Quick proxy without a grant: `codesign -v -R="<designated requirement of build 1>" <build 2>.app` succeeds for two identity-signed builds and fails for two ad-hoc builds.
