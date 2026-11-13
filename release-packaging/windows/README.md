# Windows package assembly status

package-windows-development.ps1 creates only local, unsigned development outputs for the portable EXE and MSIX formats. It rejects signed input, uses a fresh sibling staging directory, refuses an existing output directory, hashes the packaged executable, and writes the manifest.json shape consumed by du-release-tools. The MSIX path uses the installed Windows SDK MakeAppx.exe, unpacks its result, and checks that the executable member has the same SHA-256 as the input. The script never signs, installs, submits, or publishes an artifact.

The development MSIX has the explicit DiskUnfold.Development identity and a placeholder publisher string. It is unsigned and is not an official Store package or a user-installable release. The manifest keeps its existing Windows 11 build 22000 applicability floor. The published HLD names 25H2 and 26H2 as support targets but does not set an MSIX install floor; F15 treats out-of-table support as guidance, so the package applicability floor remains a separate contract question. MaxVersionTested is 10.0.26200.0 (Windows 11 25H2), the highest build exercised by the local x64 CLI and package-assembly checks. Those checks do not verify MSIX installation, Windows Home, ARM64, or 26H2, and are not the official support matrix. A portable-EXE start is recorded for Windows 11 25H2 Pro x64 (see Native start check). The `manifest.json` OS range of 11 25H2 through 11 26H2 records the published target range; this development run validates only Windows 11 25H2 Pro x64 and does not establish that range as verified support. The portable EXE is unsigned and does not claim the SignPath identity. Both artifacts include DEVELOPMENT-ONLY.txt; neither is a release candidate.

## Run locally

Build the GUI launcher on the native Windows architecture after building the web assets:

    $env:CARGO_TARGET_DIR = 'C:\diskunfold-f15-package-target'
    cargo build --locked --release -p du-launcher

Then choose x64 or arm64 to match the native executable:

    $params = @{
        BinaryPath = 'target\release\diskunfold.exe'
        Version = '0.1.0'
        Architecture = 'x64'
        Format = 'portable-exe'
        OutputDirectory = 'C:\diskunfold-output\portable-x64'
    }
    & .\release-packaging\windows\package-windows-development.ps1 @params

Use Format set to msix for the unsigned MSIX development package. Its AppxManifest.xml adds the diskunfold.exe execution alias. The output directory must not already exist. The default icon comes from app-host\icons\icon.png.

test-package-windows-development.ps1 uses the supplied native executable under a new C: temporary directory, assembles both development formats, checks manifest and package-member digests, checks architecture rejection and output collision handling, then removes only its own verified temporary directory. It does not launch DiskUnfold; it checks that the portable output contains an install-form marker with the text portable.

## Official package gaps

Pass -NoticesPath with the THIRD-PARTY-NOTICES file written by build-product-resources to ship it as THIRD-PARTY-NOTICES.txt next to LICENSE (inside the package for MSIX). Add -Release to make the packager stop when -NoticesPath is missing, unreadable or empty. The Linux and macOS packagers already require --notices.

F15 HLD 116 and DLD 70 specify the MSI, portable EXE and Store MSIX formats. The architecture HLD 229 selects user-scoped MSI installation with an offline WebView2 runtime, SignPath Authenticode signing for EXE and MSI, and Store signing for MSIX. It also requires the Store publisher identity, Windows MFT helper, notices, SBOM, support table, release provenance and per-architecture native validation.

The MSI uses the WiX Toolset pinned in wix-tool.json (see Windows MSI below). SignTool.exe being installed does not establish a valid product identity or approval. This branch has no approved SignPath credentials or Store publisher identity.

The MSI is built by package-windows-msi.ps1 and is unsigned until SignPath approves the project.

The development packager does not implement the release workflow, build metadata and notices embedding, generated THIRD-PARTY-NOTICES, SBOM, SLSA provenance, support-table generation, license/package-content audit, Windows MFT helper inclusion and signed-package membership verification, SignPath signing, Store submission, or release publishing. The launcher built from this revision answers `--help`, `--version`, `version`, `notices` and `support`. The build metadata, notices and support table it embeds are still development defaults (`support` reports the table as unavailable), so release builds must be rechecked. A successful local manifest/hash check is not a release gate.

The F15 DLD points to a published architecture DLD release workflow. AIDA returned no published architecture DLD for this version, so that cross-document implementation detail remains unavailable.

## Native start check

smoke-windows-quit.ps1 (in release-packaging/launcher) starts one GUI instance with isolated data, closes its window through UI Automation and checks the exit code, leftover processes, the instance lock, the database sidecar files and an integrity check of a database copy. Add -ProbeIpc to also read app.startup_status and app.product_info through a local WebView2 debugging port; run it against the portable EXE to confirm that the app reports the Portable install form. Add -DuringScan to close the window while a scan runs. The script waits while another DiskUnfold instance is running, so only one instance exists at a time.

## Windows MSI

package-windows-msi.ps1 builds the per-user MSI (F15 FR9.HD1, install form `msi`). It is unsigned; SignPath signing follows once the SignPath Foundation approves the public repository.

| File | Purpose |
| --- | --- |
| `DiskUnfold.wxs` | WiX source. Per-user scope, no elevation and no WiX extension. |
| `package-windows-msi.ps1` | Verifies inputs, builds the MSI, opens it with `msiexec /a`, compares its contents with the inputs and writes `manifest.json`. |
| `wix-tool.json`, `install-wix.ps1` | WiX 6.0.2 as an isolated .NET tool: the package is refused unless its SHA-256 matches the pin, and NuGet packages stay under the chosen tool path. |
| `fetch-webview2.ps1` | Downloads the WebView2 Evergreen Standalone Installer from Microsoft's permanent link and checks its signature. |
| `test-package-windows-msi.ps1` | Packager tests (no install). |

Build locally from a release build of the launcher and the notices from `build-product-resources`:

    $wix = .\release-packaging\windows\install-wix.ps1 -ToolPath D:\tools
    .\release-packaging\windows\fetch-webview2.ps1 -Architecture x64 -OutputPath D:\webview2\MicrosoftEdgeWebView2RuntimeInstallerX64.exe
    .\release-packaging\windows\package-windows-msi.ps1 -BinaryPath target\release\diskunfold.exe -Version 0.1.0 -Architecture x64 `
        -NoticesPath release-resources\THIRD-PARTY-NOTICES -WebView2InstallerPath D:\webview2\MicrosoftEdgeWebView2RuntimeInstallerX64.exe `
        -WixPath $wix -OutputDirectory D:\out\msi-x64

What the MSI does:

- Installs diskunfold.exe, LICENSE, THIRD-PARTY-NOTICES.txt, WEBVIEW2-NOTICE.txt and a one-line `install-form` marker (`msi`) to `%LOCALAPPDATA%\Programs\DiskUnfold` without administrator rights, and appends that folder to the user PATH so `diskunfold` runs from a terminal. platform-os reports install form `msi` from the marker. Uninstall removes the files and the PATH entry and keeps the data folder.
- Carries the WebView2 Evergreen Standalone Installer unmodified in the Binary table. A deferred custom action runs it with `/silent /install` only when neither `HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}` nor the `HKCU` key has a `pv` value other than empty or 0.0.0.0 (Microsoft's documented check). The installer ships only inside this MSI.
- Before packaging, the packager requires a valid Authenticode signature with the subject CN=Microsoft Corporation on the installer, and the file name Microsoft uses for the architecture (both standalone installers are 32-bit wrappers, so the PE header cannot tell them apart). It records the installer's SHA-256 and version in the manifest row (`bundled_components`), which flows into the release inventory and `DISTRIBUTION.md`. The digest is not pinned because Microsoft replaces the file.
- Licensing: THIRD-PARTY-NOTICES ends with a WebView2 section (generated from `release-tools/data`) with Microsoft's license terms and the SmartScreen notice, and the MSI installs the same text as WEBVIEW2-NOTICE.txt. The WebView2 Runtime is governed by Microsoft's license and DiskUnfold is not endorsed by Microsoft. The license text in `release-tools/data/webview2-license.txt` is a copy that needs a legal review against the terms Microsoft shows on its download page.

`package-windows-msi.ps1` accepts two test-only switches (`-TestOnlyWebView2ClientKey`, `-TestOnlySkipWebView2Signature`) that let an installer test point the runtime probe at a missing key and embed a stub. They are refused unless used together and never write a manifest.
