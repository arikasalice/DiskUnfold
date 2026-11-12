# macOS DMG packaging foundation

`package-dmg.sh` accepts a prebuilt universal `diskunfold` executable and creates a new output directory containing a drag-install DMG, the signed executable staged for digest verification, and a one-row `manifest.json` accepted by `du-release-tools`.

The bundle is `DiskUnfold.app/Contents` with `MacOS/diskunfold`, `Resources/DiskUnfold.icns`, `Info.plist`, and `PkgInfo`. The DMG contains that app and an `/Applications` symlink. The script requires exactly the `arm64` and `x86_64` slices, signs the bundle ad-hoc with Hardened Runtime and an empty entitlements dictionary, verifies the mounted app, and checks that the executable copied from the DMG has the same SHA-256 as the signed bundle executable.

The output directory must not exist; use a new task-specific staging path. The script does not overwrite existing files. Its manifest records the complete DMG SHA-256, the packaged executable SHA-256, and the limited scope of an ad-hoc signature. It does not create SLSA provenance, attest to a publisher identity, notarize, publish, build the executable, or install the app.

The bundle layout and signed executable are stable for the same input, but repeated `hdiutil` runs produced different DMG bytes in native verification. The manifest hashes each exact DMG; this foundation does not claim bit-for-bit disk-image reproducibility.

Pass `--bundle-id` explicitly. The published architecture HLD requires a reverse-DNS bundle identifier to be selected and kept stable, but does not specify its value. The packaging test uses `org.example.diskunfold.packagingtest` only as a test placeholder.

Run `test-package-dmg.sh` on macOS to build thin test binaries for both architectures, assemble a universal binary, create and verify the DMG, inspect the release manifest, check the app signature and metadata, and test collision and thin-binary rejection. It does not launch the GUI.

The feature HLD's CLI menu link still requires app-host menu integration. Packaging can stage the app bundle but cannot add an application menu action without changing that product code. The current release inventory tool also validates a staged executable digest separately; the package script independently mounts the DMG and checks the executable member digest before emitting its manifest.
