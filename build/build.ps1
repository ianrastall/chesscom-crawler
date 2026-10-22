<#
.SYNOPSIS
    Checks, tests and publishes Chess.com Crawler as a portable win-x64 folder and zip.

.EXAMPLE
    pwsh build/build.ps1                 # full check, tests and Release publish
    pwsh build/build.ps1 -SkipChecks     # publish only
#>
param(
    [switch]$SkipChecks
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$target = Join-Path $root 'target'
$dist = Join-Path $root 'dist'
$version = ([xml](Get-Content (Join-Path $root 'Directory.Build.props'))).Project.PropertyGroup.Version | Select-Object -First 1
$appDir = Join-Path $dist 'ChesscomCrawler'

function Invoke-Step([string]$Name, [scriptblock]$Command) {
    Write-Host "== $Name" -ForegroundColor Cyan
    & $Command
    if ($LASTEXITCODE -ne 0) { throw "$Name failed (exit $LASTEXITCODE)." }
}

Push-Location $root
try {
    if (-not $SkipChecks) {
        Invoke-Step 'cargo fmt' { cargo fmt --all --check }
        Invoke-Step 'cargo clippy' { cargo clippy --workspace --all-targets --locked --target-dir $target -- -D warnings }
        Invoke-Step 'cargo test' { cargo test --workspace --locked --target-dir $target }
        Invoke-Step 'interop tests' { dotnet run --project tests/ChesscomCrawler.InteropTests -c Release }
    }

    if (Test-Path $appDir) { Remove-Item $appDir -Recurse -Force }
    Invoke-Step 'publish app' { dotnet publish src/ChesscomCrawler.App -c Release -o $appDir }
    Invoke-Step 'build CLI' { cargo build -p chesscom-crawler --release --locked --target-dir $target }
    Copy-Item (Join-Path $target 'x86_64-pc-windows-msvc/release/chesscom-crawler.exe') $appDir

    foreach ($file in 'ChesscomCrawler.exe', 'chesscom_crawler_ffi.dll', 'chesscom-crawler.exe', 'Assets/app.ico') {
        if (-not (Test-Path (Join-Path $appDir $file))) { throw "Published folder is missing $file." }
    }

    $zip = Join-Path $dist "ChesscomCrawler-$version-win-x64.zip"
    if (Test-Path $zip) { Remove-Item $zip -Force }
    Compress-Archive -Path (Join-Path $appDir '*') -DestinationPath $zip
    Write-Host "Published $appDir and $zip" -ForegroundColor Green
}
finally {
    Pop-Location
}
