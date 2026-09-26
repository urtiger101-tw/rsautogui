#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$InstallDir,
    [string]$HomeDirectory = $env:USERPROFILE,
    [string[]]$SkillHosts = @(),
    [string[]]$McpHosts = @(),
    [string[]]$CustomSkillRoots = @(),
    [string]$CodexHome,
    [string]$OpenCodeHome,
    [string]$ClaudeConfig,
    [string]$AgyConfig,
    [switch]$NonInteractive,
    [switch]$Preview
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2
$utf8 = New-Object System.Text.UTF8Encoding($false)
$package = $PSScriptRoot
function Full([string]$Path) { [IO.Path]::GetFullPath($Path) }
function WriteUtf8([string]$Path, [string]$Text) { [IO.File]::WriteAllText($Path, $Text, $utf8) }
function Hash([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash }
function Hosts([string[]]$Items) {
    @($Items | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim().ToLowerInvariant() } | Where-Object { $_ } | Select-Object -Unique)
}
if (-not $NonInteractive -and -not $PSBoundParameters.ContainsKey('SkillHosts') -and -not $PSBoundParameters.ContainsKey('McpHosts')) {
    Write-Host 'AutoGui: EXE + on-demand DLL (all functionality included).'
    Write-Host 'Hosts: codex, claude, opencode, agy. Blank = program only.'
    $SkillHosts = @((Read-Host 'Install Skill for (comma separated)'))
    $McpHosts = @((Read-Host 'Register MCP for (comma separated)'))
}
$SkillHosts = @(Hosts $SkillHosts)
$McpHosts = @(Hosts $McpHosts)
foreach ($name in @($SkillHosts) + @($McpHosts)) {
    if ($name -notin @('codex','claude','opencode','agy')) { throw "Unsupported host '$name'. Use -CustomSkillRoots for other agents." }
}
$HomeDirectory = Full $HomeDirectory
if (-not $InstallDir) {
    $local = if ($HomeDirectory -eq $env:USERPROFILE) { $env:LOCALAPPDATA } else { Join-Path $HomeDirectory 'AppData\Local' }
    $InstallDir = Join-Path $local 'Programs\AutoGui'
}
$InstallDir = Full $InstallDir
if (-not $CodexHome) { $CodexHome = if ($HomeDirectory -eq $env:USERPROFILE -and $env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $HomeDirectory '.codex' } }
if (-not $OpenCodeHome) { $OpenCodeHome = if ($HomeDirectory -eq $env:USERPROFILE -and $env:XDG_CONFIG_HOME) { Join-Path $env:XDG_CONFIG_HOME 'opencode' } else { Join-Path $HomeDirectory '.config\opencode' } }
if (-not $ClaudeConfig) { $ClaudeConfig = Join-Path $HomeDirectory '.claude.json' }
if (-not $AgyConfig) { $AgyConfig = Join-Path $HomeDirectory '.gemini\config\mcp_config.json' }
$roots = @{
    codex = Join-Path $CodexHome 'skills'
    claude = Join-Path $HomeDirectory '.claude\skills'
    opencode = Join-Path $OpenCodeHome 'skills'
    agy = Join-Path $HomeDirectory '.gemini\antigravity-cli\skills'
}
$openConfig = Join-Path $OpenCodeHome 'opencode.json'
if (Test-Path -LiteralPath (Join-Path $OpenCodeHome 'opencode.jsonc')) { $openConfig = Join-Path $OpenCodeHome 'opencode.jsonc' }
$configs = @{codex=(Join-Path $CodexHome 'config.toml'); claude=$ClaudeConfig; opencode=$openConfig; agy=$AgyConfig}
$exe = Join-Path $InstallDir 'autogui-control.exe'
$manifestPath = Join-Path $InstallDir '.install\manifest.json'
if (Test-Path -LiteralPath $manifestPath) { throw 'An AutoGui installation exists. Run its uninstall.ps1 first; modified files are preserved.' }
if ((Test-Path -LiteralPath $InstallDir) -and @(Get-ChildItem -LiteralPath $InstallDir -Recurse -File -Force).Count) { throw "Install directory must contain no existing files: $InstallDir" }

$payload = Get-Content -LiteralPath (Join-Path $package 'payload.json') -Raw | ConvertFrom-Json
$copies = New-Object System.Collections.ArrayList
foreach ($item in $payload.files) {
    $source = Full (Join-Path $package $item.path)
    if (-not $source.StartsWith((Full $package).TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid payload path' }
    if ((Hash $source) -ne $item.sha256) { throw "Package hash mismatch: $($item.path)" }
    [void]$copies.Add(@{Source=$source; Path=(Full (Join-Path $InstallDir $item.path)); Text=$null})
}
$skillSource = Join-Path $package 'skills\autogui-control'
$selectedRoots = @($SkillHosts | ForEach-Object { $roots[$_] }) + @($CustomSkillRoots)
foreach ($root in @($selectedRoots | Select-Object -Unique)) {
    $target = Full (Join-Path $root 'autogui-control')
    if (Test-Path -LiteralPath $target) { throw "Skill already exists; preserved: $target" }
    foreach ($file in Get-ChildItem -LiteralPath $skillSource -Recurse -File) {
        $relative = $file.FullName.Substring($skillSource.Length).TrimStart('\')
        [void]$copies.Add(@{Source=$file.FullName; Path=(Join-Path $target $relative); Text=$null})
    }
    [void]$copies.Add(@{Source=$null; Path=(Join-Path $target 'runtime.json'); Text=(@{executable=$exe; mcp_args=@('mcp')} | ConvertTo-Json)})
}
if (-not ('AutoGuiJson' -as [type])) { Add-Type -Path (Join-Path $package 'ConfigEditor.cs') }
$changes = New-Object System.Collections.ArrayList
$codexCommand = $null
foreach ($name in $McpHosts) {
    $path = Full $configs[$name]
    $before = if (Test-Path -LiteralPath $path) { [IO.File]::ReadAllText($path) } else { '{}' }
    if ($name -eq 'codex') {
        $codexCommand = Get-Command codex -ErrorAction Stop
        $previous = $env:CODEX_HOME
        try {
            $env:CODEX_HOME = Full $CodexHome
            # Read-only preflight; output can contain user data, so never log it.
            $listing = (& $codexCommand.Source mcp list --json 2>&1 | Out-String)
            if ($LASTEXITCODE -ne 0) { throw 'Codex config could not be read. Fix it before installation.' }
            $entries = $listing | ConvertFrom-Json
            if (@($entries | Where-Object { $_.name -eq 'autogui' }).Count) { throw 'Codex already has an autogui MCP server; preserved.' }
        } finally { $env:CODEX_HOME = $previous }
        $updated = $null
    } else {
        $definition = if ($name -eq 'opencode') { @{type='local';command=@($exe,'mcp');enabled=$true} } else { @{command=$exe;args=@('mcp')} }
        $container = if ($name -eq 'opencode') { 'mcp' } else { 'mcpServers' }
        $updated = [AutoGuiJson]::AddServer($before, $container, 'autogui', ($definition | ConvertTo-Json -Compress))
    }
    [void]$changes.Add(@{Host=$name; Path=$path; Text=$updated; BeforeHash=$(if (Test-Path -LiteralPath $path) { Hash $path } else { $null })})
}
Write-Host "Program: $InstallDir"
Write-Host "Skills: $($SkillHosts -join ', ')   MCP: $($McpHosts -join ', ')"
foreach ($change in $changes) { Write-Host "MCP config: $($change.Path)" }
if ($Preview) { Write-Host 'Preview only; no install/config writes.'; return }

$records = New-Object System.Collections.ArrayList
$backupDir = Join-Path $InstallDir '.install\backups'
New-Item -ItemType Directory -Path $backupDir -Force | Out-Null
function SaveManifest {
    $data = @{version=1;install_dir=$InstallDir;skill_hosts=$SkillHosts;mcp_hosts=$McpHosts;files=@($records.ToArray())}
    WriteUtf8 $manifestPath ($data | ConvertTo-Json -Depth 10)
}
try {
    foreach ($copy in $copies) {
        if (Test-Path -LiteralPath $copy.Path) { throw "Refusing to overwrite $($copy.Path)" }
        New-Item -ItemType Directory -Path (Split-Path -Parent $copy.Path) -Force | Out-Null
        if ($null -ne $copy.Text) { WriteUtf8 $copy.Path $copy.Text } else { Copy-Item -LiteralPath $copy.Source -Destination $copy.Path }
        [void]$records.Add(@{path=$copy.Path;sha256=(Hash $copy.Path);backup=$null})
        SaveManifest
    }
    foreach ($change in $changes) {
        $current = if (Test-Path -LiteralPath $change.Path) { Hash $change.Path } else { $null }
        if ($current -ne $change.BeforeHash) { throw "Config changed during installation; preserved: $($change.Path)" }
        $backup = $null
        if ($null -ne $current) {
            $backup = Join-Path $backupDir "$($change.Host).original"
            Copy-Item -LiteralPath $change.Path -Destination $backup
        }
        New-Item -ItemType Directory -Path (Split-Path -Parent $change.Path) -Force | Out-Null
        try {
            if ($change.Host -eq 'codex') {
                $previous = $env:CODEX_HOME
                try {
                    $env:CODEX_HOME = Full $CodexHome
                    $null = & $codexCommand.Source mcp add autogui -- $exe mcp 2>&1
                    if ($LASTEXITCODE -ne 0) { throw 'Codex MCP registration failed.' }
                } finally { $env:CODEX_HOME = $previous }
            } else { WriteUtf8 $change.Path $change.Text }
        } finally {
            if (Test-Path -LiteralPath $change.Path) {
                [void]$records.Add(@{path=$change.Path;sha256=(Hash $change.Path);backup=$backup})
                SaveManifest
            }
        }
    }
    Write-Host 'Installed. Restart selected agents to discover the Skill/MCP tools.'
    Write-Host "Run: & '$exe' --help"
    Write-Host "Uninstall: & '$InstallDir\uninstall.ps1'"
} catch {
    SaveManifest
    Write-Warning 'Installation failed. Rolling back owned files; changed files will be preserved.'
    & (Join-Path $package 'uninstall.ps1') -InstallDir $InstallDir
    throw
}
