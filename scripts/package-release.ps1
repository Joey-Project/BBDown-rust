param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryPath,

    [Parameter(Mandatory = $true)]
    [string]$PackageName,

    [Parameter(Mandatory = $true)]
    [string]$OutputDir
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($PackageName -notmatch '^[A-Za-z0-9._-]+$') {
    throw "package name must contain only letters, numbers, dot, underscore, or dash: $PackageName"
}

if (-not (Test-Path -LiteralPath $BinaryPath -PathType Leaf)) {
    throw "binary path does not exist: $BinaryPath"
}

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent $scriptDir
$outputFullPath = [System.IO.Path]::GetFullPath($OutputDir)
New-Item -ItemType Directory -Path $outputFullPath -Force | Out-Null

$stagingParent = Join-Path ([System.IO.Path]::GetTempPath()) ("bbdown-release-" + [System.Guid]::NewGuid().ToString("N"))
$stagingDir = Join-Path $stagingParent $PackageName
$archivePath = Join-Path $outputFullPath "$PackageName.zip"
$checksumPath = "$archivePath.sha256"

try {
    New-Item -ItemType Directory -Path (Join-Path $stagingDir "docs/architecture") -Force | Out-Null
    Copy-Item -LiteralPath $BinaryPath -Destination (Join-Path $stagingDir "bbdown.exe")
    Copy-Item -LiteralPath (Join-Path $repoRoot "README.md") -Destination (Join-Path $stagingDir "README.md")
    Copy-Item -LiteralPath (Join-Path $repoRoot "README.zh-CN.md") -Destination (Join-Path $stagingDir "README.zh-CN.md")
    Copy-Item -LiteralPath (Join-Path $repoRoot "docs/embedding.md") -Destination (Join-Path $stagingDir "docs/embedding.md")
    Copy-Item -LiteralPath (Join-Path $repoRoot "docs/embedding.zh-CN.md") -Destination (Join-Path $stagingDir "docs/embedding.zh-CN.md")
    Copy-Item -LiteralPath (Join-Path $repoRoot "docs/user-guide.md") -Destination (Join-Path $stagingDir "docs/user-guide.md")
    Copy-Item -LiteralPath (Join-Path $repoRoot "docs/user-guide.zh-CN.md") -Destination (Join-Path $stagingDir "docs/user-guide.zh-CN.md")
    Copy-Item -LiteralPath (Join-Path $repoRoot "docs/release.md") -Destination (Join-Path $stagingDir "docs/release.md")
    Copy-Item -LiteralPath (Join-Path $repoRoot "docs/release.zh-CN.md") -Destination (Join-Path $stagingDir "docs/release.zh-CN.md")
    Copy-Item -LiteralPath (Join-Path $repoRoot "docs/architecture/rust-rewrite.md") -Destination (Join-Path $stagingDir "docs/architecture/rust-rewrite.md")
    Copy-Item -LiteralPath (Join-Path $repoRoot "docs/architecture/rust-rewrite.zh-CN.md") -Destination (Join-Path $stagingDir "docs/architecture/rust-rewrite.zh-CN.md")
    $licensePath = Join-Path $repoRoot "LICENSE"
    if (Test-Path -LiteralPath $licensePath -PathType Leaf) {
        Copy-Item -LiteralPath $licensePath -Destination (Join-Path $stagingDir "LICENSE")
    }

    if (Test-Path -LiteralPath $archivePath) {
        Remove-Item -LiteralPath $archivePath -Force
    }
    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $epoch = [DateTimeOffset]::FromUnixTimeSeconds(0)
    $zip = [System.IO.Compression.ZipFile]::Open($archivePath, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        $files = Get-ChildItem -LiteralPath $stagingDir -Recurse -File | Sort-Object FullName
        foreach ($file in $files) {
            $relativePath = [System.IO.Path]::GetRelativePath($stagingParent, $file.FullName).Replace("\", "/")
            $entry = $zip.CreateEntry($relativePath, [System.IO.Compression.CompressionLevel]::Optimal)
            $entry.LastWriteTime = $epoch
            $inputStream = [System.IO.File]::OpenRead($file.FullName)
            try {
                $outputStream = $entry.Open()
                try {
                    $inputStream.CopyTo($outputStream)
                }
                finally {
                    $outputStream.Dispose()
                }
            }
            finally {
                $inputStream.Dispose()
            }
        }
    }
    finally {
        $zip.Dispose()
    }
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant()
    $archiveName = Split-Path -Leaf $archivePath
    Set-Content -LiteralPath $checksumPath -Value "$hash  $archiveName" -Encoding ascii
    Write-Output $archivePath
}
finally {
    if (Test-Path -LiteralPath $stagingParent) {
        Remove-Item -LiteralPath $stagingParent -Recurse -Force
    }
}
