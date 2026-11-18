#requires -Version 5.1
[CmdletBinding()]
param([string]$OutputDir, [switch]$SkipBuild, [switch]$Installer, [string]$IsccPath)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
function Read-CrateVersion([string]$Path) {
    $text = [IO.File]::ReadAllText($Path)
    if ($text -match '(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"') { return $Matches[1] }
    throw "Cannot read [package] version from $Path"
}
$version = Read-CrateVersion (Join-Path $repo 'Cargo.toml')
foreach ($crate in @('launcher', 'runtime')) {
    $other = Read-CrateVersion (Join-Path $repo "$crate\Cargo.toml")
    if ($other -ne $version) { Write-Warning "$crate version $other differs from root version $version" }
}
function Find-Iscc {
    if ($IsccPath) {
        if (-not (Test-Path -LiteralPath $IsccPath -PathType Leaf)) { throw "ISCC not found at -IsccPath: $IsccPath" }
        return (Resolve-Path -LiteralPath $IsccPath).Path
    }
    $candidates = @(
        $env:ISCC,
        $(if (${env:ProgramFiles(x86)}) { Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe' }),
        $(if ($env:ProgramFiles) { Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe' }),
        $(if ($env:LOCALAPPDATA) { Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe' })
    )
    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path -LiteralPath $candidate -PathType Leaf)) { return (Resolve-Path -LiteralPath $candidate).Path }
    }
    $command = Get-Command iscc -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    throw 'Inno Setup 6 compiler (ISCC.exe) not found. Install Inno Setup 6, set $env:ISCC, or pass -IsccPath.'
}
# Resolve the compiler before the slow build so a missing tool fails early.
$iscc = if ($Installer) { Find-Iscc } else { $null }
if (-not $OutputDir) { $OutputDir = Join-Path $repo ('dist\autogui-agent-' + (Get-Date -Format 'yyyyMMdd-HHmmss')) }
$OutputDir = [IO.Path]::GetFullPath($OutputDir)
if (Test-Path -LiteralPath $OutputDir) { throw "Output already exists: $OutputDir" }
if (-not $SkipBuild) {
    Push-Location $repo
    try { & cargo build --release --workspace --all-features --locked; if ($LASTEXITCODE -ne 0) { throw 'Build failed' } }
    finally { Pop-Location }
}
New-Item -ItemType Directory -Path $OutputDir | Out-Null
foreach ($name in @('autogui-control.exe','autogui_runtime.dll')) {
    Copy-Item -LiteralPath (Join-Path $repo "target\release\$name") -Destination $OutputDir
}
foreach ($name in @('install.ps1','uninstall.ps1','install.cmd','ConfigEditor.cs')) {
    Copy-Item -LiteralPath (Join-Path $repo "installer\$name") -Destination $OutputDir
}
Copy-Item -LiteralPath (Join-Path $repo 'skills') -Destination $OutputDir -Recurse
New-Item -ItemType Directory -Path (Join-Path $OutputDir 'docs') | Out-Null
foreach ($name in @('agent-installation.md','validation-agent.md','recognition-app-control.md','screensaver.md','virtual-display.md','validation-virtual-display.md','headless.md')) {
    Copy-Item -LiteralPath (Join-Path $repo "docs\$name") -Destination (Join-Path $OutputDir 'docs')
}
New-Item -ItemType Directory -Path (Join-Path $OutputDir 'scripts') | Out-Null
foreach ($name in @('virtual-display.ps1','rdp-to-console.ps1')) {
    Copy-Item -LiteralPath (Join-Path $repo "scripts\$name") -Destination (Join-Path $OutputDir 'scripts')
}
Copy-Item -LiteralPath (Join-Path $repo 'examples\support\mcp_tools.json') -Destination (Join-Path $OutputDir 'docs\mcp-tools.json')
Copy-Item -LiteralPath (Join-Path $repo 'README.md') -Destination $OutputDir
$files = @(Get-ChildItem -LiteralPath $OutputDir -Recurse -File | ForEach-Object {
    @{path=$_.FullName.Substring($OutputDir.Length + 1);sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash;bytes=$_.Length}
})
$utf8 = New-Object System.Text.UTF8Encoding($false)
[IO.File]::WriteAllText((Join-Path $OutputDir 'payload.json'), (@{version=1;files=$files} | ConvertTo-Json -Depth 6), $utf8)
$sums = @($files | ForEach-Object { "$($_.sha256)  $($_.path)" })
$sums += "$((Get-FileHash -LiteralPath (Join-Path $OutputDir 'payload.json') -Algorithm SHA256).Hash)  payload.json"
[IO.File]::WriteAllLines((Join-Path $OutputDir 'SHA256SUMS.txt'), $sums, $utf8)
Compress-Archive -Path (Join-Path $OutputDir '*') -DestinationPath "$OutputDir.zip"
Get-Item -LiteralPath "$OutputDir.zip" | Select-Object FullName,Length
if ($Installer) {
    # Setup goes next to the payload directory's parent, not inside the payload.
    $distDir = Split-Path -Parent $OutputDir
    $issPath = Join-Path $repo 'installer\autogui.iss'
    & $iscc /Qp "/DAppVersion=$version" "/DPackageDir=$OutputDir" "/DOutputDir=$distDir" $issPath
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup compilation failed (exit $LASTEXITCODE)" }
    $setup = Join-Path $distDir "AutoGui-$version-setup.exe"
    Get-Item -LiteralPath $setup | Select-Object FullName,Length
    Write-Host "SHA256 $((Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash)  $setup"
}
