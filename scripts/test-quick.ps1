<#
.SYNOPSIS
  Quick regression suite: format, lint and every test except the slow oracle/sweep tests.

.DESCRIPTION
  Target: a few seconds of test time after the release build is warm (see docs/TESTING.md).
  Skips the tests listed in scripts/tests/slow-tests.txt; the full suite (scripts/test-full.ps1)
  runs them. Needs no real data, no GPU and no network. Run from anywhere.

.PARAMETER NoLint
  Skip cargo fmt and clippy (tests only).
#>
param([switch]$NoLint)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $root
$sw = [Diagnostics.Stopwatch]::StartNew()

function Step($title, [scriptblock]$body) {
    Write-Host "== $title"
    & $body
    if ($LASTEXITCODE -ne 0) { Write-Host "QUICK SUITE: FAIL at '$title'" -ForegroundColor Red; exit 1 }
}

if (-not $NoLint) {
    Step 'cargo fmt --check' { cargo fmt --all -- --check }
    Step 'cargo clippy (default features)' { cargo clippy --workspace --all-targets -- -D warnings }
}

$skip = Get-Content (Join-Path $PSScriptRoot 'tests\slow-tests.txt') |
    Where-Object { $_ -and -not $_.StartsWith('#') } | ForEach-Object { $_.Trim() }
$skipArgs = @(); foreach ($s in $skip) { $skipArgs += '--skip'; $skipArgs += $s }

Step "cargo test --workspace --release (skipping $($skip.Count) slow tests)" {
    cargo test --workspace --release -- @skipArgs
}

Write-Host ("QUICK SUITE: PASS in {0:N0} s (real-data, GPU, SIMD and slow oracle tests are NOT part of this suite)" -f $sw.Elapsed.TotalSeconds)
