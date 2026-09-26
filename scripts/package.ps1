#requires -Version 5.1
[CmdletBinding()]
param([string]$OutputDir, [switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
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
foreach ($name in @('agent-installation.md','validation-agent.md','recognition-app-control.md','screensaver.md')) {
    Copy-Item -LiteralPath (Join-Path $repo "docs\$name") -Destination (Join-Path $OutputDir 'docs')
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
