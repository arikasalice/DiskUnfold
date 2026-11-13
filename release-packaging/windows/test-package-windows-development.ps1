#Requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $BinaryPath,

    [ValidateSet('x64', 'arm64')]
    [string] $Architecture = 'x64'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-Equal {
    param([string] $Expected, [string] $Actual, [string] $Message)
    if ($Expected -cne $Actual) {
        throw "$Message Expected '$Expected', got '$Actual'."
    }
}

function Assert-True {
    param([bool] $Condition, [string] $Message)
    if (-not $Condition) {
        throw $Message
    }
}

function Assert-TestPathHasNoReparsePoint {
    param([Parameter(Mandatory = $true)][string] $Path, [Parameter(Mandatory = $true)][string] $Description)

    $current = [System.IO.Path]::GetFullPath($Path)
    while (-not [string]::IsNullOrWhiteSpace($current)) {
        if (Test-Path -LiteralPath $current) {
            $item = Get-Item -LiteralPath $current -ErrorAction Stop
            if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "$Description contains a reparse point: $current"
            }
        }
        $parent = [System.IO.Directory]::GetParent($current)
        if (-not $parent) {
            break
        }
        $current = $parent.FullName
    }
}

function Invoke-Packager {
    param([string] $Format, [string] $OutputDirectory, [string] $RequestedArchitecture = $Architecture)

    $parameters = @{
        BinaryPath = $BinaryPath
        Version = '0.1.0'
        Architecture = $RequestedArchitecture
        Format = $Format
        OutputDirectory = $OutputDirectory
    }
    & $packageScript @parameters | Out-Null
}

function Assert-ExpectedFailure {
    param([scriptblock] $Action, [string] $Message, [string] $ExpectedMessagePattern)

    $failed = $false
    try {
        & $Action
    }
    catch {
        if ($ExpectedMessagePattern -and $_.Exception.Message -notmatch $ExpectedMessagePattern) {
            throw "$Message Expected error matching '$ExpectedMessagePattern', got '$($_.Exception.Message)'."
        }
        $failed = $true
    }
    if (-not $failed) {
        throw $Message
    }
}

$tempParent = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
if ([System.IO.Path]::GetPathRoot($tempParent) -ne 'C:\') {
    throw "The test requires a C: temporary directory; got $tempParent"
}
Assert-TestPathHasNoReparsePoint -Path $tempParent -Description 'Temporary test parent'
$packageScript = Join-Path $PSScriptRoot 'package-windows-development.ps1'
$resolvedBinary = [System.IO.Path]::GetFullPath($BinaryPath)
if (-not (Test-Path -LiteralPath $resolvedBinary -PathType Leaf)) {
    throw "Test executable does not exist: $resolvedBinary"
}

$testName = 'diskunfold-package-test-{0}' -f [guid]::NewGuid().ToString('N')
$testRoot = Join-Path $tempParent $testName
if (Test-Path -LiteralPath $testRoot) {
    throw "Refusing to reuse an existing test directory: $testRoot"
}
[System.IO.Directory]::CreateDirectory($testRoot) | Out-Null
$testRootFull = [System.IO.Path]::GetFullPath($testRoot)
$testParentFull = [System.IO.Path]::GetFullPath($tempParent).TrimEnd([System.IO.Path]::DirectorySeparatorChar)
$created = $false

try {
    $testItem = Get-Item -LiteralPath $testRootFull -ErrorAction Stop
    if ($testItem.PSIsContainer -and (($testItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -eq 0) -and
        [System.IO.Directory]::GetParent($testRootFull).FullName.TrimEnd([System.IO.Path]::DirectorySeparatorChar) -eq $testParentFull) {
        $created = $true
    }
    else {
        throw "Could not verify the isolated test directory: $testRootFull"
    }

    $portableOutput = Join-Path $testRootFull 'portable-x64'
    Invoke-Packager -Format 'portable-exe' -OutputDirectory $portableOutput
    $portableManifest = Get-Content -LiteralPath (Join-Path $portableOutput 'manifest.json') -Raw | ConvertFrom-Json
    if ($portableManifest.rows.Count -ne 1) { throw 'Portable manifest must have one row.' }
    $portableRow = $portableManifest.rows[0]
    Assert-Equal "DiskUnfold-0.1.0-$Architecture.exe" $portableRow.file_name 'Portable filename mismatch.'
    Assert-Equal 'portable-exe' $portableRow.format 'Portable format mismatch.'
    Assert-Equal 'unsigned' $portableRow.signature 'Portable signature must remain unsigned.'
    Assert-True ($portableRow.signature_scope -match 'development-only') 'Portable manifest is missing its development signature scope.'
    $portableFile = Join-Path $portableOutput $portableRow.file_name
    $portableDigest = (Get-FileHash -LiteralPath $portableFile -Algorithm SHA256).Hash.ToLowerInvariant()
    Assert-Equal $portableRow.sha256 $portableDigest 'Portable package SHA-256 mismatch.'
    Assert-Equal $portableRow.executable_sha256 $portableDigest 'Portable executable SHA-256 mismatch.'
    Assert-True (Test-Path -LiteralPath (Join-Path $portableOutput 'DEVELOPMENT-ONLY.txt') -PathType Leaf) 'Portable development marker is missing.'
    Assert-Equal 'portable' ((Get-Content -LiteralPath (Join-Path $portableOutput 'install-form') -Raw).Trim()) 'Portable install-form marker is missing or wrong.'
    Assert-True ((Get-Content -LiteralPath (Join-Path $portableOutput 'DEVELOPMENT-ONLY.txt') -Raw) -match 'Do not publish') 'Portable development marker does not forbid publishing.'

    $collisionOutput = Join-Path $testRootFull 'collision'
    [System.IO.Directory]::CreateDirectory($collisionOutput) | Out-Null
    $sentinelPath = Join-Path $collisionOutput 'keep.txt'
    [System.IO.File]::WriteAllText($sentinelPath, 'preserve')
    Assert-ExpectedFailure { Invoke-Packager -Format 'portable-exe' -OutputDirectory $collisionOutput } 'Packager replaced an existing output directory.' 'Output directory already exists'
    Assert-Equal 'preserve' (Get-Content -LiteralPath $sentinelPath -Raw) 'Output collision changed existing data.'

    $wrongArchitecture = if ($Architecture -eq 'x64') { 'arm64' } else { 'x64' }
    Assert-ExpectedFailure { Invoke-Packager -Format 'portable-exe' -OutputDirectory (Join-Path $testRootFull 'wrong-architecture') -RequestedArchitecture $wrongArchitecture } 'Packager accepted an executable for the wrong architecture.' 'Executable architecture is'

    $junctionTarget = Join-Path $testRootFull 'junction-target'
    $junctionTargetParent = Join-Path $junctionTarget 'nested-parent'
    [System.IO.Directory]::CreateDirectory($junctionTargetParent) | Out-Null
    $junctionPath = Join-Path $testRootFull 'junction-parent'
    $junctionCreated = $false
    try {
        New-Item -ItemType Junction -Path $junctionPath -Target $junctionTarget | Out-Null
        $junctionCreated = $true
        $junctionOutput = Join-Path (Join-Path $junctionPath 'nested-parent') 'junction-output'
        $junctionSentinel = Join-Path $junctionTargetParent 'keep.txt'
        [System.IO.File]::WriteAllText($junctionSentinel, 'preserve')
        Assert-ExpectedFailure { Invoke-Packager -Format 'portable-exe' -OutputDirectory $junctionOutput } 'Packager accepted an output path below an ancestor junction.' 'reparse point'
        Assert-Equal 'preserve' (Get-Content -LiteralPath $junctionSentinel -Raw) 'Junction output check changed the target sentinel.'
        Assert-True (-not (Test-Path -LiteralPath $junctionOutput)) 'Junction output check created files through the junction.'
    }
    finally {
        if ($junctionCreated -and (Test-Path -LiteralPath $junctionPath -PathType Container)) {
            Remove-Item -LiteralPath $junctionPath -Force
        }
    }

    $msixOutput = Join-Path $testRootFull 'msix'
    Invoke-Packager -Format 'msix' -OutputDirectory $msixOutput
    $msixManifest = Get-Content -LiteralPath (Join-Path $msixOutput 'manifest.json') -Raw | ConvertFrom-Json
    if ($msixManifest.rows.Count -ne 1) { throw 'MSIX manifest must have one row.' }
    $msixRow = $msixManifest.rows[0]
    Assert-Equal "DiskUnfold-0.1.0-$Architecture.msix" $msixRow.file_name 'MSIX filename mismatch.'
    Assert-Equal 'msix' $msixRow.format 'MSIX format mismatch.'
    Assert-Equal 'unsigned' $msixRow.signature 'MSIX manifest must record unsigned development status.'
    Assert-True ($msixRow.signature_scope -match 'placeholder development identity') 'MSIX manifest is missing its development identity scope.'
    $msixFile = Join-Path $msixOutput $msixRow.file_name
    $msixDigest = (Get-FileHash -LiteralPath $msixFile -Algorithm SHA256).Hash.ToLowerInvariant()
    Assert-Equal $msixRow.sha256 $msixDigest 'MSIX package SHA-256 mismatch.'
    $verificationExe = Join-Path $msixOutput 'verification\diskunfold.exe'
    $verificationDigest = (Get-FileHash -LiteralPath $verificationExe -Algorithm SHA256).Hash.ToLowerInvariant()
    Assert-Equal $msixRow.executable_sha256 $verificationDigest 'MSIX executable SHA-256 mismatch.'
    $authenticode = Get-AuthenticodeSignature -FilePath $msixFile
    Assert-Equal 'NotSigned' $authenticode.Status.ToString() 'MSIX must not receive an ad-hoc signature.'
    Assert-True (Test-Path -LiteralPath (Join-Path $msixOutput 'DEVELOPMENT-ONLY.txt') -PathType Leaf) 'MSIX development marker is missing.'

    $programFilesX86 = [System.Environment]::GetEnvironmentVariable('ProgramFiles(x86)')
    $sdkBin = Join-Path $programFilesX86 'Windows Kits\10\bin'
    $makeAppx = Get-ChildItem -LiteralPath $sdkBin -Directory -Filter '10.*' |
        Sort-Object { [version] $_.Name } -Descending |
        ForEach-Object { Join-Path $_.FullName 'x64\makeappx.exe' } |
        Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } |
        Select-Object -First 1
    if (-not $makeAppx) {
        throw 'MakeAppx.exe is required for the MSIX verification test.'
    }
    $unpacked = Join-Path $testRootFull 'msix-unpacked'
    $unpackOutput = @(& $makeAppx unpack /p $msixFile /d $unpacked 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw ("Could not unpack test MSIX: {0}" -f ($unpackOutput -join [Environment]::NewLine))
    }
    $appxManifest = Get-Content -LiteralPath (Join-Path $unpacked 'AppxManifest.xml') -Raw
    $parsedAppxManifest = [xml]$appxManifest
    $namespaceManager = [System.Xml.XmlNamespaceManager]::new($parsedAppxManifest.NameTable)
    $namespaceManager.AddNamespace('appx', 'http://schemas.microsoft.com/appx/manifest/foundation/windows10')
    $deviceFamily = $parsedAppxManifest.SelectSingleNode('/appx:Package/appx:Dependencies/appx:TargetDeviceFamily', $namespaceManager)
    if (-not $deviceFamily) {
        throw 'MSIX target device family declaration is missing.'
    }
    Assert-Equal '10.0.22000.0' $deviceFamily.GetAttribute('MinVersion') 'MSIX minimum applicability version changed.'
    Assert-Equal '10.0.26200.0' $deviceFamily.GetAttribute('MaxVersionTested') 'MSIX tested version does not match the highest exercised platform.'
    Assert-True ($appxManifest -match 'Name="DiskUnfold\.Development"') 'MSIX does not use the development-only identity.'
    Assert-True ($appxManifest -match 'Publisher="CN=DiskUnfold Development"') 'MSIX does not use the declared development publisher.'
    Assert-True ($appxManifest -match 'Alias="diskunfold\.exe"') 'MSIX CLI execution alias is missing.'
    Assert-True ($msixRow.signature_scope -match 'not Store signed') 'MSIX manifest does not explain the missing Store signature.'

    Write-Output "Windows package development checks passed for $Architecture."
    Write-Output "Portable SHA-256: $portableDigest"
    Write-Output "Unsigned MSIX SHA-256: $msixDigest"
}
finally {
    if ($created -and (Test-Path -LiteralPath $testRootFull -PathType Container)) {
        $cleanupItem = Get-Item -LiteralPath $testRootFull -ErrorAction Stop
        $actualTestRoot = [System.IO.Path]::GetFullPath($cleanupItem.FullName)
        $actualParent = [System.IO.Directory]::GetParent($actualTestRoot).FullName.TrimEnd([System.IO.Path]::DirectorySeparatorChar)
        $cleanupPathIsRegular = (($cleanupItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -eq 0)
        $cleanupParentIsRegular = $false
        try {
            Assert-TestPathHasNoReparsePoint -Path $tempParent -Description 'Temporary test parent'
            $cleanupParentIsRegular = $true
        }
        catch {
            Write-Warning "Preserving test directory because its parent changed: $($_.Exception.Message)"
        }
        if ($cleanupItem.PSIsContainer -and $cleanupPathIsRegular -and $cleanupParentIsRegular -and
            $actualTestRoot -eq $testRootFull -and $actualParent -eq $testParentFull -and
            (Split-Path -Leaf $actualTestRoot) -eq $testName) {
            Remove-Item -LiteralPath $actualTestRoot -Recurse -Force
        }
        else {
            Write-Warning "Preserving test directory because its identity changed: $testRootFull"
        }
    }
}
