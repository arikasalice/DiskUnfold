# Windows package assembly status

package-windows-development.ps1 creates only local, unsigned development outputs for the portable EXE and MSIX formats. It rejects signed input, uses a fresh sibling staging directory, refuses an existing output directory, hashes the packaged executable, and writes the manifest.json shape consumed by du-release-tools. The MSIX path uses the installed Windows SDK MakeAppx.exe, unpacks its result, and checks that the executable member has the same SHA-256 as the input. The script never signs, installs, submits, or publishes an artifact.

The development MSIX has the explicit DiskUnfold.Development identity and a placeholder publisher string. It is unsigned and is not an official Store package or a user-installable release. The portable EXE is unsigned and does not claim the SignPath identity. Both artifacts include DEVELOPMENT-ONLY.txt; neither is a release candidate.

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

test-package-windows-development.ps1 uses the supplied native executable under a new C: temporary directory, assembles both development formats, checks manifest and package-member digests, checks architecture rejection and output collision handling, then removes only its own verified temporary directory. It does not launch DiskUnfold.

## Official package gaps

F15 HLD 116 and DLD 70 specify the MSI, portable EXE and Store MSIX formats. The architecture HLD 229 selects user-scoped MSI installation with an offline WebView2 runtime, SignPath Authenticode signing for EXE and MSI, and Store signing for MSIX. It also requires the Store publisher identity, Windows MFT helper, notices, SBOM, support table, release provenance and per-architecture native validation.

The current environment has Windows SDK 10.0.26100 MakeAppx.exe and SignTool.exe, MSVC Build Tools, Cargo and .NET. It has no WiX authoring tools (wix, candle, light) and this branch has no approved SignPath credentials or Store publisher identity. SignTool.exe being installed does not establish a valid product identity or approval. msiexec.exe can install MSI files; it does not author one.

The MSI is intentionally not emitted because this environment lacks the selected MSI authoring tool and the approved Microsoft WebView2 offline runtime installer with a verified version and digest. No runtime is downloaded or copied by this package script. The Store package identity is intentionally a development placeholder, and the MSIX remains unsigned; no Store submission or user installation is supported. The F15 DLD says a SignPath timeout is a failed release gate and must not be replaced by an unsigned package.

The development packager does not implement the release workflow, build metadata and notices embedding, generated THIRD-PARTY-NOTICES, SBOM, SLSA provenance, support-table generation, license/package-content audit, SignPath signing, Store submission, or release publishing. A successful local manifest/hash check is not a release gate.

The F15 DLD points to a published architecture DLD release workflow. AIDA returned no published architecture DLD for this version, so that cross-document implementation detail remains unavailable.
