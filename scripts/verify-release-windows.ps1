param(
    [string]$DistDirectory = "dist"
)

$ErrorActionPreference = "Stop"
$archive = Get-ChildItem -LiteralPath $DistDirectory -Filter "bellhound-*.zip" | Select-Object -First 1
if (-not $archive) { throw "Bellhound archive not found in $DistDirectory" }
$checksum = "$($archive.FullName).sha256"
if (-not (Test-Path -LiteralPath $checksum)) { throw "Checksum not found: $checksum" }

$expected = (Get-Content -LiteralPath $checksum -Raw).Split([char[]]" `t`r`n", [System.StringSplitOptions]::RemoveEmptyEntries)[0].ToLowerInvariant()
$actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive.FullName).Hash.ToLowerInvariant()
if ($actual -ne $expected) { throw "Bellhound checksum mismatch" }

$stage = Join-Path ([System.IO.Path]::GetTempPath()) ("bellhound-verify-" + [guid]::NewGuid())
try {
    Expand-Archive -LiteralPath $archive.FullName -DestinationPath $stage
    $root = Get-ChildItem -LiteralPath $stage -Directory | Select-Object -First 1
    if (-not $root) { throw "Archive root not found" }
    foreach ($path in @("bellhound.exe", "INSTALL.md", "README.md", "LICENSE", "AUTHORS.md", "CHANGELOG.md", "CONTRIBUTING.md", "SECURITY.md", "bellhound.example.toml", "docs/platform-validation.md", "examples/crocodile-call.jsonl", "examples/mcp-status-only.toml", "scripts/verify-package-behavior.py")) {
        if (-not (Test-Path -LiteralPath (Join-Path $root.FullName $path))) {
            throw "Archive member not found: $path"
        }
    }
    & (Join-Path $root.FullName "bellhound.exe") --version
    if ($LASTEXITCODE -ne 0) { throw "Bellhound version check failed" }
    & (Join-Path $root.FullName "bellhound.exe") --config (Join-Path $root.FullName "bellhound.example.toml") check
    if ($LASTEXITCODE -ne 0) { throw "Bellhound configuration check failed" }
    python (Join-Path $root.FullName "scripts/verify-package-behavior.py") --binary (Join-Path $root.FullName "bellhound.exe") --root $root.FullName
    if ($LASTEXITCODE -ne 0) { throw "Bellhound packaged behavior check failed" }
    Write-Output "verified $($archive.Name)"
}
finally {
    if (Test-Path -LiteralPath $stage) {
        Remove-Item -LiteralPath $stage -Recurse -Force
    }
}
