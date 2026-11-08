<#
.SYNOPSIS
  Full regression suite: everything in the quick suite plus slow oracle tests, all-feature
  (GPU) build, real-data and hardware gates, PixInsight adapter tests and the CLI smoke run.

.DESCRIPTION
  Runs on the owner's computer only (GitHub Actions stay disabled). Needs the DS-B frames in
  "SeestarS50 Subs", an AVX2 CPU and a GPU adapter. Real-data/SIMD/GPU tests silently pass
  when their environment variables are unset, so this script sets them and refuses to start
  if the data is missing (use -AllowMissingData to run the rest anyway; the result is then
  reported as INCOMPLETE, never as PASS).

  Exit code 0 = PASS, 1 = FAIL, 2 = INCOMPLETE (a gate could not run).
  See docs/TESTING.md.

.PARAMETER AllowMissingData
  Continue without the DS-B frames; the final verdict is INCOMPLETE.
.PARAMETER SkipSmoke
  Skip the CLI smoke run (scripts/test-cli-smoke.ps1).
#>
param([switch]$AllowMissingData, [switch]$SkipSmoke)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $root
$sw = [Diagnostics.Stopwatch]::StartNew()
$incomplete = @()

# RAM preflight: builds with LTO plus a tile-stacking test run need headroom.
$os = Get-CimInstance Win32_OperatingSystem
$freeGb = [math]::Round($os.FreePhysicalMemory / 1MB, 1)
Write-Host "Free RAM: $freeGb GB"
if ($freeGb -lt 6) { Write-Host "Less than 6 GB RAM free; close other programs first." -ForegroundColor Red; exit 1 }

$dir = Join-Path $root 'SeestarS50 Subs'
$frame = Join-Path $dir 'Light_M 33_10.0s_IRCUT_20261006-204534.fit'
if ((Test-Path $dir) -and (Test-Path $frame)) {
    $env:FULGURSTACK_TEST_FITS_DIR = $dir
    $env:FULGURSTACK_TEST_FITS = $frame
} elseif ($AllowMissingData) {
    $incomplete += 'real-data gates (DS-B frames missing)'
} else {
    Write-Host "DS-B frames not found in '$dir'. Real-data gates would silently pass; aborting." -ForegroundColor Red
    exit 1
}
$env:FULGURSTACK_TEST_SIMD = '1'   # require AVX2 kernels to run, not skip
$env:FULGURSTACK_TEST_GPU = '1'    # require a real GPU adapter, never a silent skip

function Step($title, [scriptblock]$body) {
    Write-Host "== $title"
    & $body
    if ($LASTEXITCODE -ne 0) { Write-Host "FULL SUITE: FAIL at '$title'" -ForegroundColor Red; exit 1 }
}

Step 'cargo fmt --check' { cargo fmt --all -- --check }
Step 'cargo clippy (default features)' { cargo clippy --workspace --all-targets -- -D warnings }
Step 'cargo clippy (all features, GPU)' { cargo clippy --workspace --all-targets --all-features -- -D warnings }
Step 'cargo test --workspace --release (default features, all tests)' { cargo test --workspace --release }
Step 'cargo test --workspace --release --all-features (GPU parity, real data, SIMD)' { cargo test --workspace --release --all-features }

if (Get-Command node -ErrorAction SilentlyContinue) {
    Step 'node --test (PixInsight adapter contract)' { node --test scripts/tests/open-fulgurstack.test.cjs }
} else { $incomplete += 'node missing: PixInsight adapter tests not run' }

if (-not $SkipSmoke) {
    Step 'cargo build --release -p fulgur-cli' { cargo build --release -p fulgur-cli }
    if (Test-Path $dir) {
        Step 'CLI smoke on DS-B (scripts/test-cli-smoke.ps1)' { & (Join-Path $PSScriptRoot 'test-cli-smoke.ps1'); }
    } else { $incomplete += 'CLI smoke skipped (no DS-B frames)' }
}

Write-Host ("Elapsed {0:N0} s" -f $sw.Elapsed.TotalSeconds)
if ($incomplete.Count) {
    Write-Host "FULL SUITE: INCOMPLETE" -ForegroundColor Yellow
    $incomplete | ForEach-Object { Write-Host "  - $_" }
    exit 2
}
Write-Host "FULL SUITE: PASS"
exit 0
