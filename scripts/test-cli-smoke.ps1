<#
.SYNOPSIS
  CLI smoke test on the 32-frame DS-B set (release build, read-only inputs).

.DESCRIPTION
  Runs every user-facing command once, checks exit codes, bounded RAM, the privacy and
  immutability rules of the contract (no GPS in reports or console, no temporary or
  intermediate files, inputs byte-identical) and, when Python/NumPy and the PixInsight
  reference master are present, the DS-B quality comparison with PixInsight.
  Outputs go to a new folder under outputs/ (git-ignored); nothing is written next to the inputs.

  Exit code 0 = PASS, 1 = at least one check failed.
  See docs/TESTING.md (suite F-CLI).

.PARAMETER Subs
  Folder with the DS-B frames (default: "SeestarS50 Subs" in the repository).

.PARAMETER Strict
  Also fail on known, documented defects (docs/TESTING.md, section "Known defects").
#>
param(
    [string]$Subs = '',
    [switch]$Strict
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not $Subs) { $Subs = Join-Path $root 'SeestarS50 Subs' }
$exe = Join-Path $root 'target\release\fulgurstack.exe'
if (-not (Test-Path $exe)) { throw "Missing $exe; run: cargo build --release -p fulgur-cli" }
if (-not (Test-Path $Subs)) { throw "Missing test frames: $Subs" }

$stamp = Get-Date -Format 'yyyyMMdd_HHmmss'
$out = Join-Path $root "outputs\test_cli_smoke_$stamp"
New-Item -ItemType Directory $out | Out-Null
$one = Get-ChildItem $Subs -File | Where-Object { $_.Name -like 'Light_M 33_10.0s_IRCUT_20261006-204534.fit' } | Select-Object -First 1
if (-not $one) { $one = Get-ChildItem $Subs -File | Select-Object -First 1 }

$script:failed = @()
$script:known = @()
function Check($ok, $what) {
    if ($ok) { Write-Host "  PASS  $what" } else { Write-Host "  FAIL  $what" -ForegroundColor Red; $script:failed += $what }
}
function KnownDefect($ok, $id, $what) {
    if ($ok) { Write-Host "  PASS  $id fixed: $what" }
    elseif ($Strict) { Write-Host "  FAIL  $id $what" -ForegroundColor Red; $script:failed += "$id $what" }
    else { Write-Host "  KNOWN $id $what" -ForegroundColor Yellow; $script:known += "$id $what" }
}

function Run([string]$name, [string[]]$cliArgs) {
    $quoted = $cliArgs | ForEach-Object { if ($_ -match '\s') { '"' + $_ + '"' } else { $_ } }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $p = Start-Process -FilePath $exe -ArgumentList ($quoted -join ' ') -NoNewWindow -PassThru `
        -RedirectStandardOutput (Join-Path $out "$name.out.txt") -RedirectStandardError (Join-Path $out "$name.err.txt")
    $peak = 0
    while (-not $p.HasExited) {
        try { $p.Refresh(); if ($p.PeakWorkingSet64 -gt $peak) { $peak = $p.PeakWorkingSet64 } } catch {}
        Start-Sleep -Milliseconds 100
    }
    $p.WaitForExit()
    $sw.Stop()
    $text = (Get-Content (Join-Path $out "$name.out.txt") -Raw) + (Get-Content (Join-Path $out "$name.err.txt") -Raw)
    Write-Host ("[{0}] exit={1} wall={2:N1}s peakRAM={3:N0} MB" -f $name, $p.ExitCode, $sw.Elapsed.TotalSeconds, ($peak / 1MB))
    [pscustomobject]@{ Exit = $p.ExitCode; Text = $text; Seconds = $sw.Elapsed.TotalSeconds; PeakMB = $peak / 1MB }
}

function FitsHeader([string]$path) {
    $fs = [IO.File]::OpenRead($path)
    try { $buf = [byte[]]::new(2880 * 4); [void]$fs.Read($buf, 0, $buf.Length) } finally { $fs.Dispose() }
    [Text.Encoding]::ASCII.GetString($buf)
}

$hashBefore = (Get-ChildItem $Subs -File | Get-FileHash -Algorithm SHA256 | ForEach-Object { $_.Hash }) -join ''

Write-Host "== survey"
$r = Run 'survey' @('survey', '--input', $Subs, '--report', "$out\survey.json")
Check ($r.Exit -eq 0) 'survey exits 0'
Check (Test-Path "$out\survey.json") 'survey report written'

Write-Host "== debayer-single"
$r = Run 'debayer' @('debayer-single', '--input', $one.FullName, '--output', "$out\single.fits")
Check ($r.Exit -eq 0) 'debayer-single exits 0'
$h = FitsHeader "$out\single.fits"
Check ($h -match 'BITPIX\s*=\s*-32') 'single-frame export is BITPIX -32'
Check ($h -match 'NAXIS3\s*=\s*3') 'single-frame export is a 3-plane cube'

Write-Host "== inspect"
$r = Run 'inspect' @('inspect', '--input', $Subs, '--report', "$out\quality.json")
Check ($r.Exit -eq 0) 'inspect exits 0'
Check ($r.Text -match '32 frames inspected') 'inspect saw all 32 frames'
Check ($r.Seconds -lt 15) 'inspect of 32 frames well inside the M2 budget (<15 s)'

Write-Host "== register"
$r = Run 'register' @('register', '--input', $Subs, '--report', "$out\register.json")
Check ($r.Exit -eq 0) 'register exits 0'
Check (-not (Get-ChildItem $out -Filter '*.fit*' | Where-Object { $_.Name -like 'register*' })) 'register writes no image files'

Write-Host "== stack (auto rejection, FITS)"
$r = Run 'stack' @('stack', '--input', $Subs, '--output', "$out\master.fits", '--report', "$out\master_report.json")
Check ($r.Exit -eq 0) 'stack exits 0'
Check ($r.Text -match 'PASS: 2160 x 3840 RGB master from (\d+)/32 frames') 'stack reports accepted/total frames'
Check ($r.PeakMB -lt 12288) 'peak RAM below the 12 GiB ceiling'
$h = FitsHeader "$out\master.fits"
Check ($h -match 'BITPIX\s*=\s*-32') 'master is BITPIX -32'
Check ($h -match 'NCOMBINE\s*=\s*\d+') 'master carries NCOMBINE'
Check ($h -match 'TEXPTIME') 'master carries TEXPTIME'
Check ($h -match 'SITELAT') 'reference-anchor GPS preserved in the master (owner rule)'
Check ((Test-Path "$out\master_coverage.fits") -and (Test-Path "$out\master_high_rejection.fits")) 'coverage and high-rejection maps written'
$rep = Get-Content "$out\master_report.json" -Raw
Check ($rep -notmatch 'SITELAT|SITELONG') 'report JSON holds no GPS'
Check ($r.Text -notmatch 'SITELAT|SITELONG') 'console holds no GPS'
$method = (ConvertFrom-Json $rep).integration.rejection   # method actually applied (config.rejection may say "auto")
$card = ([regex]::Match($h, "REJECT\s*=\s*'([^']*)'")).Groups[1].Value.Trim()
Check ($card -ieq ($method -replace 'winsorized','WINSOR-MAD' -replace 'linear-fit','LINEAR-FIT')) "FITS REJECT card ('$card') names the method actually used ('$method')"
$appVersion = (& $exe --version) -replace '^\S+\s+', ''
Check ($h -match ("CREATOR\s*=\s*'FulgurStack " + [regex]::Escape($appVersion))) 'FITS CREATOR card carries the application version'

Write-Host "== stack (ESD, XISF output)"
$r = Run 'stack_xisf' @('stack', '--input', $Subs, '--rejection', 'esd', '--output', "$out\master_esd.xisf", '--report', "$out\master_esd_report.json")
Check ($r.Exit -eq 0) 'XISF master exits 0'
Check ((Get-Item "$out\master_esd.xisf").Length -gt 90MB) 'XISF master has its full float payload'

Write-Host "== stack (median, coverage-optimized canvas)"
$r = Run 'stack_median' @('stack', '--input', $Subs, '--rejection', 'median', '--canvas', 'coverage-optimized', '--output', "$out\master_med.fits", '--report', "$out\master_med_report.json")
Check ($r.Exit -eq 0) 'median coverage-optimized stack exits 0'
Check ((Get-Item "$out\master_med.fits").Length -lt (Get-Item "$out\master.fits").Length) 'coverage-optimized canvas is smaller than the reference canvas'

Write-Host "== refusals"
$r = Run 'bad_input' @('stack', '--input', "$out\nonexistent", '--output', "$out\x.fits", '--report', "$out\x.json")
Check ($r.Exit -ne 0 -and $r.Text -match 'FAIL') 'missing input fails with a diagnostic'
Check (-not (Test-Path "$out\x.fits")) 'no output after a failed run'
$before = (Get-FileHash "$out\master.fits").Hash
$r = Run 'exists' @('stack', '--input', $Subs, '--output', "$out\master.fits", '--report', "$out\again.json")
Check ($r.Exit -ne 0 -and $r.Text -match 'already exists') 'existing output is refused'
Check ((Get-FileHash "$out\master.fits").Hash -eq $before) 'existing output is untouched'
$r = Run 'bad_tile' @('stack', '--input', $Subs, '--tile-size', '300', '--output', "$out\t.fits", '--report', "$out\t.json")
Check ($r.Exit -ne 0) 'invalid tile size is refused'

Write-Host "== contract: no intermediates, immutable inputs"
Check (-not (Get-ChildItem $out -Recurse -Include '*.tmp' -ErrorAction SilentlyContinue)) 'no .tmp files left behind'
$fits = Get-ChildItem $out -Filter '*.fits' | ForEach-Object { $_.Name }
$allowed = 'single.fits', 'master.fits', 'master_coverage.fits', 'master_high_rejection.fits', 'master_med.fits', 'master_med_coverage.fits', 'master_med_high_rejection.fits'
Check (-not ($fits | Where-Object { $allowed -notcontains $_ })) 'only masters and their maps were written'
$hashAfter = (Get-ChildItem $Subs -File | Get-FileHash -Algorithm SHA256 | ForEach-Object { $_.Hash }) -join ''
Check ($hashBefore -eq $hashAfter) 'input frames are byte-identical after all runs'

Write-Host "== PixInsight reference comparison (DS-B)"
$pi = Join-Path $root 'PixInsight Files\integration.fit'
$py = Get-Command python -ErrorAction SilentlyContinue
if ((Test-Path $pi) -and $py) {
    & python -c "import numpy" 2>$null
    if ($LASTEXITCODE -eq 0) {
        & python (Join-Path $root 'scripts\tests\compare-master.py') --master "$out\master.fits" --registration-report "$out\master_report.json" --reference $pi --report "$out\compare.json" | Out-Null
        $cmp = Get-Content "$out\compare.json" -Raw | ConvertFrom-Json
        Check ($cmp.status -eq 'PASS') "quality versus PixInsight master: $($cmp.status)"
    } else { Write-Host "  SKIP  NumPy missing" -ForegroundColor Yellow }
} else { Write-Host "  SKIP  PixInsight reference or Python missing" -ForegroundColor Yellow }

Write-Host ""
if ($script:known.Count) { Write-Host "Known defects still open: $($script:known.Count)" -ForegroundColor Yellow; $script:known | ForEach-Object { Write-Host "  - $_" } }
if ($script:failed.Count) {
    Write-Host "CLI SMOKE: FAIL ($($script:failed.Count) checks)" -ForegroundColor Red
    $script:failed | ForEach-Object { Write-Host "  - $_" }
    exit 1
}
Write-Host "CLI SMOKE: PASS (outputs: $out)"
exit 0
