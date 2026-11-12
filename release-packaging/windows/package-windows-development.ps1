#Requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $BinaryPath,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+\.\d+\.\d+(?:\.\d+)?$')]
    [string] $Version,

    [Parameter(Mandatory = $true)]
    [ValidateSet('x64', 'arm64')]
    [string] $Architecture,

    [Parameter(Mandatory = $true)]
    [ValidateSet('portable-exe', 'msix')]
    [string] $Format,

    [Parameter(Mandatory = $true)]
    [string] $OutputDirectory,

    [string] $IconPath = (Join-Path $PSScriptRoot '..\..\app-host\icons\icon.png')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-RegularFile {
    param([Parameter(Mandatory = $true)][string] $Path, [Parameter(Mandatory = $true)][string] $Description)

    $resolved = [System.IO.Path]::GetFullPath($Path)
    $item = Get-Item -LiteralPath $resolved -ErrorAction Stop
    if ($item.PSIsContainer -or (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "$Description must be a regular file, not a directory or reparse point: $resolved"
    }
    return $resolved
}

function Get-PeArchitecture {
    param([Parameter(Mandatory = $true)][string] $Path)

    $stream = $null
    $reader = $null
    try {
        $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
        if ($stream.Length -lt 70) {
            throw 'The executable is too short to be a valid PE file.'
        }
        $reader = [System.IO.BinaryReader]::new($stream)
        if ($reader.ReadByte() -ne 0x4d -or $reader.ReadByte() -ne 0x5a) {
            throw 'The input does not have a DOS MZ header.'
        }
        $stream.Position = 0x3c
        $peOffset = $reader.ReadInt32()
        if ($peOffset -lt 64 -or ($peOffset + 6) -gt $stream.Length) {
            throw 'The PE header offset is invalid.'
        }
        $stream.Position = $peOffset
        $signature = $reader.ReadBytes(4)
        if ($signature.Length -ne 4 -or $signature[0] -ne 0x50 -or $signature[1] -ne 0x45 -or $signature[2] -ne 0 -or $signature[3] -ne 0) {
            throw 'The input does not have a valid PE signature.'
        }
        $machine = $reader.ReadUInt16()
        switch ($machine) {
            0x8664 { return 'x64' }
            0xaa64 { return 'arm64' }
            default { throw ('Unsupported PE machine value: 0x{0:x4}' -f $machine) }
        }
    }
    finally {
        if ($reader) { $reader.Dispose() } elseif ($stream) { $stream.Dispose() }
    }
}

function Write-Utf8NoBom {
    param([Parameter(Mandatory = $true)][string] $Path, [Parameter(Mandatory = $true)][string] $Contents)

    [System.IO.File]::WriteAllText($Path, $Contents, [System.Text.UTF8Encoding]::new($false))
}

function New-ScaledPng {
    param(
        [Parameter(Mandatory = $true)][string] $Source,
        [Parameter(Mandatory = $true)][string] $Destination,
        [Parameter(Mandatory = $true)][int] $Size
    )

    $image = $null
    $bitmap = $null
    $graphics = $null
    try {
        $image = [System.Drawing.Image]::FromFile($Source)
        if ($image.Width -ne $image.Height -or $image.Width -lt $Size) {
            throw "The MSIX source icon must be square and at least $Size pixels."
        }
        $bitmap = [System.Drawing.Bitmap]::new($Size, $Size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        $graphics.Clear([System.Drawing.Color]::Transparent)
        $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $graphics.DrawImage($image, 0, 0, $Size, $Size)
        $bitmap.Save($Destination, [System.Drawing.Imaging.ImageFormat]::Png)
    }
    finally {
        if ($graphics) { $graphics.Dispose() }
        if ($bitmap) { $bitmap.Dispose() }
        if ($image) { $image.Dispose() }
    }
}

function Find-MakeAppx {
    $programFilesX86 = [System.Environment]::GetEnvironmentVariable('ProgramFiles(x86)')
    if ([string]::IsNullOrWhiteSpace($programFilesX86)) {
        throw 'Windows SDK location is unavailable; MakeAppx.exe is required for MSIX development packaging.'
    }
    $sdkRoot = Join-Path $programFilesX86 'Windows Kits\10\bin'
    if (-not (Test-Path -LiteralPath $sdkRoot -PathType Container)) {
        throw "Windows SDK MakeAppx.exe was not found under $sdkRoot."
    }
    $versions = @(Get-ChildItem -LiteralPath $sdkRoot -Directory -Filter '10.*' | Sort-Object { [version] $_.Name } -Descending)
    foreach ($versionDirectory in $versions) {
        $candidate = Join-Path $versionDirectory.FullName 'x64\makeappx.exe'
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            return $candidate
        }
    }
    throw 'Windows SDK MakeAppx.exe was not found in any installed SDK version.'
}

function Invoke-MakeAppx {
    param(
        [Parameter(Mandatory = $true)][string] $Executable,
        [Parameter(Mandatory = $true)][string[]] $Arguments
    )

    $output = @(& $Executable @Arguments 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw ("MakeAppx failed with exit code {0}: {1}" -f $LASTEXITCODE, ($output -join [Environment]::NewLine))
    }
}

function Remove-GeneratedDirectory {
    param(
        [Parameter(Mandatory = $true)][string] $Path,
        [Parameter(Mandatory = $true)][string] $ExpectedParent
    )

    $resolved = [System.IO.Path]::GetFullPath($Path)
    $parent = [System.IO.Path]::GetFullPath($ExpectedParent).TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
    if ([System.IO.Directory]::GetParent($resolved).FullName.TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar) -ne $parent) {
        throw "Refusing to remove a generated directory outside its expected parent: $resolved"
    }
    if (Test-Path -LiteralPath $resolved -PathType Container) {
        $item = Get-Item -LiteralPath $resolved -ErrorAction Stop
        if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Refusing to remove a generated reparse point: $resolved"
        }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}

function Get-PackageVersion {
    param([Parameter(Mandatory = $true)][string] $Value)

    $parsed = [version]::Parse($Value)
    if ($parsed.Major -gt 65535 -or $parsed.Minor -gt 65535 -or $parsed.Build -gt 65535 -or ($parsed.Revision -gt 65535)) {
        throw "MSIX version components must be from 0 through 65535: $Value"
    }
    if ($parsed.Revision -lt 0) {
        return '{0}.{1}.{2}.0' -f $parsed.Major, $parsed.Minor, $parsed.Build
    }
    return $parsed.ToString(4)
}

function New-MsixManifest {
    param(
        [Parameter(Mandatory = $true)][string] $PackageVersion,
        [Parameter(Mandatory = $true)][string] $PackageArchitecture
    )

    return @"
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10" xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10" xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities" xmlns:uap5="http://schemas.microsoft.com/appx/manifest/uap/windows10/5" IgnorableNamespaces="uap rescap uap5">
  <Identity Name="DiskUnfold.Development" Publisher="CN=DiskUnfold Development" Version="$PackageVersion" ProcessorArchitecture="$PackageArchitecture" />
  <Properties>
    <DisplayName>DiskUnfold Development</DisplayName>
    <PublisherDisplayName>DiskUnfold Development</PublisherDisplayName>
    <Description>Unsigned development package. Not an official DiskUnfold release.</Description>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.22000.0" MaxVersionTested="10.0.26100.0" />
  </Dependencies>
  <Resources>
    <Resource Language="en-US" />
  </Resources>
  <Applications>
    <Application Id="DiskUnfold" Executable="diskunfold.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements DisplayName="DiskUnfold Development" Description="Unsigned development package. Not for Store submission." BackgroundColor="transparent" Square150x150Logo="Assets\Square150x150Logo.png" Square44x44Logo="Assets\Square44x44Logo.png">
      </uap:VisualElements>
      <Extensions>
        <uap5:Extension Category="windows.appExecutionAlias">
          <uap5:AppExecutionAlias>
            <uap5:ExecutionAlias Alias="diskunfold.exe" />
          </uap5:AppExecutionAlias>
        </uap5:Extension>
      </Extensions>
    </Application>
  </Applications>
  <Capabilities>
    <rescap:Capability Name="runFullTrust" />
  </Capabilities>
</Package>
"@
}

$sourceBinary = Get-RegularFile -Path $BinaryPath -Description 'Executable input'
$licensePath = Get-RegularFile -Path (Join-Path $PSScriptRoot '..\..\LICENSE') -Description 'Project license'
$actualArchitecture = Get-PeArchitecture -Path $sourceBinary
if ($actualArchitecture -ne $Architecture) {
    throw "Executable architecture is $actualArchitecture but $Architecture was requested."
}
$signature = Get-AuthenticodeSignature -FilePath $sourceBinary
if ($signature.Status -ne [System.Management.Automation.SignatureStatus]::NotSigned) {
    throw "Only unsigned development inputs are accepted; Authenticode status is $($signature.Status)."
}
if ($Format -eq 'msix') {
    $sourceIcon = Get-RegularFile -Path $IconPath -Description 'MSIX icon input'
    Add-Type -AssemblyName System.Drawing
    $msixVersion = Get-PackageVersion -Value $Version
    $makeAppx = Find-MakeAppx
}
else {
    $msixVersion = $null
    $makeAppx = $null
}

$formatExtension = if ($Format -eq 'msix') { 'msix' } else { 'exe' }
$artifactFileName = 'DiskUnfold-{0}-{1}.{2}' -f $Version, $Architecture, $formatExtension
$executableFileName = if ($Format -eq 'portable-exe') { $artifactFileName } else { 'diskunfold.exe' }
$minimumOs = '11 25H2'
$maximumOs = '11 26H2'
$signatureScope = if ($Format -eq 'msix') {
    'Unsigned development-only MSIX with a placeholder development identity; not Store signed or installable as an official package.'
}
else {
    'Unsigned development-only executable; no SignPath certificate or publisher identity has been applied or validated.'
}
$runInstructions = if ($Format -eq 'msix') {
    'Development only: not for user installation or Store submission. The Store publisher identity and Store signature are not set.'
}
else {
    'Development only: run this executable directly. The WebView2 runtime must already be available; this artifact is not an official release.'
}

$outputFullPath = [System.IO.Path]::GetFullPath($OutputDirectory)
$outputParent = [System.IO.Directory]::GetParent($outputFullPath)
if (-not $outputParent -or -not (Test-Path -LiteralPath $outputParent.FullName -PathType Container)) {
    throw 'The output parent directory must already exist.'
}
$parentItem = Get-Item -LiteralPath $outputParent.FullName -ErrorAction Stop
if (($parentItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw 'The output parent directory cannot be a reparse point.'
}
if (Test-Path -LiteralPath $outputFullPath) {
    throw "Output directory already exists; it will not be replaced: $outputFullPath"
}

$stageName = '.diskunfold-development-{0}' -f [guid]::NewGuid().ToString('N')
$stagePath = Join-Path $outputParent.FullName $stageName
$stageFullPath = [System.IO.Path]::GetFullPath($stagePath)
$parentPrefix = $outputParent.FullName.TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
if (-not $stageFullPath.StartsWith($parentPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Generated staging path escaped the selected output parent.'
}
[System.IO.Directory]::CreateDirectory($stageFullPath) | Out-Null
$published = $false

try {
    $artifactPath = Join-Path $stageFullPath $artifactFileName
    $developmentNotice = @"
DEVELOPMENT ARTIFACT ONLY
This package is unsigned and is not an official DiskUnfold release. Do not publish it or present it as Store signed or Authenticode signed.

The F15 official release pipeline, publisher identities, signing approvals, generated THIRD-PARTY-NOTICES, SBOM, build provenance, and support table are not provided by this development packager. MSI packaging and the offline WebView2 runtime are not included.
"@
    if ($Format -eq 'portable-exe') {
        Copy-Item -LiteralPath $sourceBinary -Destination $artifactPath
        Copy-Item -LiteralPath $licensePath -Destination (Join-Path $stageFullPath 'LICENSE')
        Write-Utf8NoBom -Path (Join-Path $stageFullPath 'DEVELOPMENT-ONLY.txt') -Contents $developmentNotice
        $stagedExecutablePath = $artifactPath
    }
    else {
        $packageRoot = Join-Path $stageFullPath 'msix-root'
        [System.IO.Directory]::CreateDirectory($packageRoot) | Out-Null
        Copy-Item -LiteralPath $sourceBinary -Destination (Join-Path $packageRoot 'diskunfold.exe')
        Copy-Item -LiteralPath $licensePath -Destination (Join-Path $packageRoot 'LICENSE')
        $noticePath = Join-Path $packageRoot 'DEVELOPMENT-ONLY.txt'
        Write-Utf8NoBom -Path $noticePath -Contents $developmentNotice

        $assetsPath = Join-Path $packageRoot 'Assets'
        [System.IO.Directory]::CreateDirectory($assetsPath) | Out-Null
        New-ScaledPng -Source $sourceIcon -Destination (Join-Path $assetsPath 'StoreLogo.png') -Size 50
        New-ScaledPng -Source $sourceIcon -Destination (Join-Path $assetsPath 'Square44x44Logo.png') -Size 44
        New-ScaledPng -Source $sourceIcon -Destination (Join-Path $assetsPath 'Square150x150Logo.png') -Size 150
        $manifestPath = Join-Path $packageRoot 'AppxManifest.xml'
        Write-Utf8NoBom -Path $manifestPath -Contents (New-MsixManifest -PackageVersion $msixVersion -PackageArchitecture $Architecture)

        Invoke-MakeAppx -Executable $makeAppx -Arguments @('pack', '/d', $packageRoot, '/p', $artifactPath)
        $unpackPath = Join-Path $stageFullPath 'unpack-check'
        Invoke-MakeAppx -Executable $makeAppx -Arguments @('unpack', '/p', $artifactPath, '/d', $unpackPath)
        $unpackedBinary = Get-RegularFile -Path (Join-Path $unpackPath 'diskunfold.exe') -Description 'MSIX executable member'
        $sourceDigest = (Get-FileHash -LiteralPath $sourceBinary -Algorithm SHA256).Hash.ToLowerInvariant()
        $memberDigest = (Get-FileHash -LiteralPath $unpackedBinary -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($sourceDigest -ne $memberDigest) {
            throw 'MSIX executable member digest does not match the input build.'
        }
        $verifyDirectory = Join-Path $stageFullPath 'verification'
        [System.IO.Directory]::CreateDirectory($verifyDirectory) | Out-Null
        $stagedExecutablePath = Join-Path $verifyDirectory 'diskunfold.exe'
        Copy-Item -LiteralPath $unpackedBinary -Destination $stagedExecutablePath
        Remove-GeneratedDirectory -Path $unpackPath -ExpectedParent $stageFullPath
        Remove-GeneratedDirectory -Path $packageRoot -ExpectedParent $stageFullPath
        Copy-Item -LiteralPath $licensePath -Destination (Join-Path $stageFullPath 'LICENSE')
        Write-Utf8NoBom -Path (Join-Path $stageFullPath 'DEVELOPMENT-ONLY.txt') -Contents $developmentNotice
    }

    $packageDigest = (Get-FileHash -LiteralPath $artifactPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $executableDigest = (Get-FileHash -LiteralPath $stagedExecutablePath -Algorithm SHA256).Hash.ToLowerInvariant()
    $manifest = [ordered]@{
        product_name = 'DiskUnfold'
        version = $Version
        rows = @(
            [ordered]@{
                file_name = $artifactFileName
                version = $Version
                os = [ordered]@{ family = 'windows'; minimum = $minimumOs; maximum = $maximumOs }
                cpu = $Architecture
                format = $Format
                channel = 'direct'
                signature = 'unsigned'
                signature_scope = $signatureScope
                sha256 = $packageDigest
                executable_file_name = $executableFileName
                executable_sha256 = $executableDigest
                run_instructions = $runInstructions
            }
        )
        channel_references = @()
        staged_files = @(
            [ordered]@{
                artifact_file_name = $artifactFileName
                kind = 'package'
                relative_path = $artifactFileName
            },
            [ordered]@{
                artifact_file_name = $artifactFileName
                kind = 'executable'
                relative_path = if ($Format -eq 'portable-exe') { $artifactFileName } else { 'verification\diskunfold.exe' }
            }
        )
    }
    $manifestJson = ConvertTo-Json -InputObject $manifest -Depth 12
    Write-Utf8NoBom -Path (Join-Path $stageFullPath 'manifest.json') -Contents $manifestJson
    if (Test-Path -LiteralPath $outputFullPath) {
        throw "Output directory appeared during packaging; preserving it: $outputFullPath"
    }
    $currentParent = Get-Item -LiteralPath $outputParent.FullName -ErrorAction Stop
    if (($currentParent.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw 'Output parent changed to a reparse point during packaging.'
    }
    [System.IO.Directory]::Move($stageFullPath, $outputFullPath)
    $published = $true
    Write-Output "Development package: $outputFullPath"
    Write-Output "Artifact: $(Join-Path $outputFullPath $artifactFileName)"
    Write-Output "SHA-256: $packageDigest"
    Write-Output 'Signature: unsigned development artifact; do not publish.'
}
finally {
    if (-not $published -and (Test-Path -LiteralPath $stageFullPath -PathType Container)) {
        $actualStagePath = [System.IO.Path]::GetFullPath((Get-Item -LiteralPath $stageFullPath -ErrorAction Stop).FullName)
        if ($actualStagePath -eq $stageFullPath -and $actualStagePath.StartsWith($parentPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
            Remove-GeneratedDirectory -Path $actualStagePath -ExpectedParent $outputParent.FullName
        }
        else {
            Write-Warning "Preserving staging directory because its identity changed: $stageFullPath"
        }
    }
}
