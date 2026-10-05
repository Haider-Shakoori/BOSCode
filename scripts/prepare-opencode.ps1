$ErrorActionPreference = "Stop"

$Version = "v1.18.34"
$Asset = "opencode-windows-x64-baseline.zip"
$ExpectedSha256 = "f89ab2720050780a450e3cf3e48ac3f0409235b46b6c548c69aa2b7051d716f4"
$Url = "https://github.com/anomalyco/opencode/releases/download/$Version/$Asset"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$ResourceDir = Join-Path $RepoRoot "src-tauri\resources\opencode"
$BinaryPath = Join-Path $ResourceDir "opencode.exe"
$VersionPath = Join-Path $ResourceDir "VERSION"

New-Item -ItemType Directory -Force -Path $ResourceDir | Out-Null

if ((Test-Path $BinaryPath) -and (Test-Path $VersionPath)) {
    $CurrentVersion = (Get-Content $VersionPath -Raw).Trim()
    if ($CurrentVersion -eq $Version) {
        Write-Host "OpenCode $Version is already prepared for the BOSCode bundle."
        exit 0
    }
}

$TempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("boscode-opencode-" + [Guid]::NewGuid().ToString("N"))
$ArchivePath = Join-Path $TempRoot $Asset
$ExtractDir = Join-Path $TempRoot "extract"

try {
    New-Item -ItemType Directory -Force -Path $TempRoot | Out-Null

    Write-Host "Downloading OpenCode $Version for the BOSCode Windows bundle..."
    Invoke-WebRequest -Uri $Url -OutFile $ArchivePath -UseBasicParsing

    $Stream = [System.IO.File]::OpenRead($ArchivePath)
    try {
        $Sha256 = [System.Security.Cryptography.SHA256]::Create()
        try {
            $HashBytes = $Sha256.ComputeHash($Stream)
        }
        finally {
            $Sha256.Dispose()
        }
    }
    finally {
        $Stream.Dispose()
    }

    $ActualSha256 = ([System.BitConverter]::ToString($HashBytes)).Replace("-", "").ToLowerInvariant()
    if ($ActualSha256 -ne $ExpectedSha256) {
        throw "OpenCode checksum mismatch. Expected $ExpectedSha256 but received $ActualSha256."
    }

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::ExtractToDirectory($ArchivePath, $ExtractDir)

    $Candidate = Get-ChildItem -Path $ExtractDir -Filter "opencode.exe" -File -Recurse | Select-Object -First 1

    if (-not $Candidate) {
        throw "The OpenCode release archive did not contain opencode.exe."
    }

    Copy-Item -Path $Candidate.FullName -Destination $BinaryPath -Force
    Set-Content -Path $VersionPath -Value $Version -Encoding ascii

    Write-Host "Prepared OpenCode $Version at $BinaryPath."
}
finally {
    if (Test-Path $TempRoot) {
        Remove-Item -Path $TempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
