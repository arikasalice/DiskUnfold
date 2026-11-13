# Helpers shared by the Windows packagers. Dot-source this file; it only defines functions.
Set-StrictMode -Version Latest

function Get-RegularFile {
    param([Parameter(Mandatory = $true)][string] $Path, [Parameter(Mandatory = $true)][string] $Description)

    $resolved = [System.IO.Path]::GetFullPath($Path)
    $item = Get-Item -LiteralPath $resolved -ErrorAction Stop
    if ($item.PSIsContainer -or (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "$Description must be a regular file, not a directory or reparse point: $resolved"
    }
    return $resolved
}

function Assert-NoReparsePointInPath {
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
    Assert-NoReparsePointInPath -Path $ExpectedParent -Description 'Generated directory parent'
    if (Test-Path -LiteralPath $resolved -PathType Container) {
        $item = Get-Item -LiteralPath $resolved -ErrorAction Stop
        if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Refusing to remove a generated reparse point: $resolved"
        }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}

