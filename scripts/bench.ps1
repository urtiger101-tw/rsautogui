#requires -Version 5.1
<#
.SYNOPSIS
  Reproducible speed benchmark for autogui-rs (recognition, capture, launcher, MCP).

.DESCRIPTION
  Builds the release workspace and the recognition_bench example, then measures:
    * recognition_bench cases (exact / confidence, RGB / grayscale, noise / UI-like)
    * primary-monitor screenshot (read-only; skipped with -NoCapture or when no desktop)
    * autogui-control.exe cold start for '--help' (average of -Runs)
    * MCP initialize + tools/list round trip on a fresh 'autogui-control mcp' process
  No mouse or keyboard input is ever sent.

.EXAMPLE
  pwsh scripts/bench.ps1                 # human-readable table
  pwsh scripts/bench.ps1 -Json           # machine-readable JSON
  pwsh scripts/bench.ps1 -NoBuild -Runs 50
#>
[CmdletBinding()]
param(
    [int]$Runs = 20,
    [int]$Iterations = 5,
    [switch]$NoBuild,
    [switch]$NoCapture,
    [switch]$Json,
    [string]$TargetDir
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $TargetDir) { $TargetDir = Join-Path $root 'target' }
$release = Join-Path $TargetDir 'release'
$exe = Join-Path $release 'autogui-control.exe'
$bench = Join-Path $release 'examples\recognition_bench.exe'

if (-not $NoBuild) {
    Push-Location $root
    try {
        cargo build --release --workspace --quiet
        if ($LASTEXITCODE -ne 0) { throw 'cargo build --release --workspace failed' }
        cargo build --release --features opencv --example recognition_bench --quiet
        if ($LASTEXITCODE -ne 0) { throw 'cargo build recognition_bench failed' }
    } finally { Pop-Location }
}
foreach ($path in @($exe, $bench)) { if (-not (Test-Path -LiteralPath $path)) { throw "missing $path (run without -NoBuild)" } }

$rows = New-Object System.Collections.Generic.List[object]
function Add-Row([string]$Group, [string]$Case, [string]$Detail, [double]$Median, [double]$Best, [int]$Count) {
    $rows.Add([pscustomobject]@{
        group = $Group; case = $Case; detail = $Detail
        median_ms = [math]::Round($Median, 2); best_ms = [math]::Round($Best, 2); runs = $Count
    })
}
function Get-Stats([double[]]$Samples) {
    $sorted = @($Samples | Sort-Object)
    @{ Median = $sorted[[int][math]::Floor($sorted.Count / 2)]; Best = $sorted[0]; Mean = ($Samples | Measure-Object -Average).Average }
}

# 1. Recognition (deterministic, in-memory) and optional read-only capture.
$benchArgs = @('--full-hd', '--json', '--iterations', "$Iterations")
if (-not $NoCapture) { $benchArgs += '--capture' }
$lines = & $bench @benchArgs 2>&1
if ($LASTEXITCODE -ne 0 -and -not $NoCapture) {
    Write-Warning "capture unavailable ($($lines | Select-Object -Last 1)); rerunning without --capture"
    $lines = & $bench --full-hd --json --iterations $Iterations
}
if ($LASTEXITCODE -ne 0) { throw "recognition_bench failed: $lines" }
foreach ($line in $lines) {
    $text = "$line"
    if (-not $text.StartsWith('{')) { continue }
    $item = $text | ConvertFrom-Json
    $group = if ($item.case -like 'screenshot*') { 'capture' } else { 'locate' }
    Add-Row $group $item.case $item.size $item.median_ms $item.best_ms $item.iterations
}

# 2. Launcher cold start: process spawn to exit for '--help'.
$samples = for ($i = 0; $i -lt $Runs; $i++) {
    $watch = [Diagnostics.Stopwatch]::StartNew()
    & $exe --help | Out-Null
    $watch.Stop()
    if ($LASTEXITCODE -ne 0) { throw "--help exited $LASTEXITCODE" }
    $watch.Elapsed.TotalMilliseconds
}
$s = Get-Stats $samples
Add-Row 'launcher' '--help cold start' ("mean {0:N2} ms" -f $s.Mean) $s.Median $s.Best $Runs

# 3. MCP: spawn 'mcp', initialize, notifications/initialized, tools/list, EOF.
function Invoke-McpRoundTrip {
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = $exe
    $start.Arguments = 'mcp'
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardOutputEncoding = [Text.Encoding]::UTF8
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $process = [Diagnostics.Process]::Start($start)
    $null = $process.StandardError.ReadToEndAsync()
    $process.StandardInput.WriteLine('{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"bench","version":"1"}}}')
    $process.StandardInput.Flush()
    $init = $process.StandardOutput.ReadLine()
    $process.StandardInput.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}')
    $process.StandardInput.WriteLine('{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}')
    $process.StandardInput.Flush()
    $list = $process.StandardOutput.ReadLine()
    $watch.Stop()
    $process.StandardInput.Close()
    if (-not $process.WaitForExit(10000)) { $process.Kill(); throw 'MCP server did not exit on EOF' }
    if (-not $init -or -not $list -or ($list | ConvertFrom-Json).result.tools.Count -lt 1) { throw "bad MCP response: $init / $list" }
    @{ Ms = $watch.Elapsed.TotalMilliseconds; Bytes = [Text.Encoding]::UTF8.GetByteCount($list) }
}
$bytes = 0
$samples = for ($i = 0; $i -lt $Runs; $i++) { $r = Invoke-McpRoundTrip; $bytes = $r.Bytes; $r.Ms }
$s = Get-Stats $samples
Add-Row 'mcp' 'initialize + tools/list' ("spawn included; tools/list $bytes B") $s.Median $s.Best $Runs

# 4. Binary sizes.
foreach ($name in @('autogui-control.exe', 'autogui_runtime.dll')) {
    $path = Join-Path $release $name
    if (Test-Path -LiteralPath $path) {
        $rows.Add([pscustomobject]@{ group = 'size'; case = $name; detail = ('{0:N0} KiB' -f ((Get-Item -LiteralPath $path).Length / 1KB)); median_ms = $null; best_ms = $null; runs = $null })
    }
}

if ($Json) {
    $rows | ConvertTo-Json -Depth 3
} else {
    $rows | Format-Table group, case, detail, @{ n = 'median ms'; e = { $_.median_ms }; a = 'right' }, @{ n = 'best ms'; e = { $_.best_ms }; a = 'right' }, runs -AutoSize | Out-String -Width 200 | Write-Output
}
