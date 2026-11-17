param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [string]$OutputDirectory = "dist"
)

$ErrorActionPreference = "Stop"
$versionMatch = Select-String -Path "Cargo.toml" -Pattern '^version = "([^"]+)"' | Select-Object -First 1
if (-not $versionMatch) { throw "Could not read package version from Cargo.toml" }
$version = $versionMatch.Matches[0].Groups[1].Value
$archiveName = "bellhound-$version-$Target"
$stageRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("bellhound-package-" + [guid]::NewGuid())
$stage = Join-Path $stageRoot $archiveName

try {
    cargo build --release --locked --target $Target
    New-Item -ItemType Directory -Force -Path $stage, $OutputDirectory | Out-Null
    Copy-Item "target/$Target/release/bellhound.exe" $stage
    Copy-Item README.md, INSTALL.md, AUTHORS.md, LICENSE, CHANGELOG.md, CONTRIBUTING.md, SECURITY.md, server.json $stage
    Copy-Item bellhound.example.toml $stage
    Copy-Item assets, docs, examples, scripts, skills -Destination $stage -Recurse
    $zip = Join-Path $OutputDirectory "$archiveName.zip"
    Compress-Archive -Path $stage -DestinationPath $zip -Force
    $hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
    "$hash  $archiveName.zip" | Set-Content -NoNewline "$zip.sha256"
    Write-Output $zip
    Write-Output "$zip.sha256"
}
finally {
    if (Test-Path -LiteralPath $stageRoot) {
        Remove-Item -LiteralPath $stageRoot -Recurse -Force
    }
}
