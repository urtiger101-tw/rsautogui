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
    [string]$ClaudeDesktopConfig,
    [string]$AgyConfig,
    [string]$AgySkillRoot,
    # Codex CLI to use for MCP registration; default: probe PATH and the standard install location.
    [string]$CodexPath,
    # Setup passes a file here; warnings (for example a skipped host) are written to it for display.
    [string]$WarningFile,
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
$warnings = New-Object System.Collections.ArrayList
function Warn([string]$Message) { Write-Warning $Message; [void]$warnings.Add($Message) }
function Hosts([string[]]$Items) {
    @($Items | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim().ToLowerInvariant() } | Where-Object { $_ } | Select-Object -Unique)
}
if (-not $NonInteractive -and -not $PSBoundParameters.ContainsKey('SkillHosts') -and -not $PSBoundParameters.ContainsKey('McpHosts')) {
    Write-Host 'AutoGui: EXE + on-demand DLL (all functionality included).'
    Write-Host 'Skill hosts: codex, claude, opencode, agy. MCP hosts: codex, claude, claude-desktop, opencode, agy. Blank = program only.'
    $SkillHosts = @((Read-Host 'Install Skill for (comma separated)'))
    $McpHosts = @((Read-Host 'Register MCP for (comma separated)'))
}
$SkillHosts = @(Hosts $SkillHosts)
$McpHosts = @(Hosts $McpHosts)
foreach ($name in $SkillHosts) {
    if ($name -eq 'claude-desktop') { throw 'Claude Desktop is MCP-only; use -McpHosts claude-desktop.' }
    if ($name -notin @('codex','claude','opencode','agy')) { throw "Unsupported Skill host '$name'. Use -CustomSkillRoots for other agents." }
}
foreach ($name in $McpHosts) {
    if ($name -notin @('codex','claude','claude-desktop','opencode','agy')) { throw "Unsupported MCP host '$name'. Use -CustomSkillRoots for other agents." }
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
if (-not $ClaudeDesktopConfig) {
    $roaming = if ($HomeDirectory -eq $env:USERPROFILE) { $env:APPDATA } else { Join-Path $HomeDirectory 'AppData\Roaming' }
    $ClaudeDesktopConfig = Join-Path $roaming 'Claude\claude_desktop_config.json'
}
if (-not $AgyConfig) { $AgyConfig = Join-Path $HomeDirectory '.gemini\config\mcp_config.json' }
if (-not $AgySkillRoot) { $AgySkillRoot = Join-Path $HomeDirectory '.gemini\config\skills' }
$roots = @{
    codex = Join-Path $CodexHome 'skills'
    claude = Join-Path $HomeDirectory '.claude\skills'
    opencode = Join-Path $OpenCodeHome 'skills'
    agy = $AgySkillRoot
}
$openConfig = Join-Path $OpenCodeHome 'opencode.json'
if (Test-Path -LiteralPath (Join-Path $OpenCodeHome 'opencode.jsonc')) { $openConfig = Join-Path $OpenCodeHome 'opencode.jsonc' }
$configs = @{codex=(Join-Path $CodexHome 'config.toml'); claude=$ClaudeConfig; 'claude-desktop'=$ClaudeDesktopConfig; opencode=$openConfig; agy=$AgyConfig}
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
# Every codex executable that might be the real CLI, in order: an explicit -CodexPath only;
# otherwise the current PATH, the persisted machine and user PATH (Setup started from
# Explorer can inherit a stale PATH), then the standard per-user install location.
function CodexCandidates {
    if ($CodexPath) { return @(Full $CodexPath) }
    $found = New-Object System.Collections.ArrayList
    foreach ($command in @(Get-Command codex -All -CommandType Application -ErrorAction SilentlyContinue)) { [void]$found.Add($command.Source) }
    $persisted = @([Environment]::GetEnvironmentVariable('Path','Machine'), [Environment]::GetEnvironmentVariable('Path','User')) | Where-Object { $_ }
    foreach ($directory in @(($persisted -join ';') -split ';' | Where-Object { $_.Trim() })) {
        foreach ($leaf in @('codex.exe','codex.cmd')) {
            try { $candidate = Join-Path ([Environment]::ExpandEnvironmentVariables($directory.Trim())) $leaf } catch { continue }
            [void]$found.Add($candidate)
        }
    }
    $localData = if ($HomeDirectory -eq $env:USERPROFILE -and $env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { Join-Path $HomeDirectory 'AppData\Local' }
    [void]$found.Add((Join-Path $localData 'Programs\OpenAI\Codex\bin\codex.exe'))
    @($found | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | ForEach-Object { Full $_ } | Select-Object -Unique)
}
# Runs one codex CLI with CODEX_HOME pointed at the selected profile. Never throws: a
# launcher shim that cannot start, or stderr output under ErrorActionPreference=Stop,
# is reported as a failed run. Output can contain user data, so it is never logged.
function InvokeCodex([string]$Command, [string[]]$Arguments) {
    $previous = $env:CODEX_HOME
    $ErrorActionPreference = 'Continue'
    try {
        $env:CODEX_HOME = Full $CodexHome
        $global:LASTEXITCODE = 0
        $lines = @(& $Command @Arguments 2>&1)
        $stdout = ($lines | Where-Object { $_ -isnot [Management.Automation.ErrorRecord] } | Out-String)
        return @{Ok=($LASTEXITCODE -eq 0); Output=$stdout}
    } catch {
        return @{Ok=$false; Output=''}
    } finally { $env:CODEX_HOME = $previous }
}
$codexCommand = $null
$codexHint = "Register it later with: codex mcp add autogui -- `"$exe`" mcp"
foreach ($name in @($McpHosts)) {
    $path = Full $configs[$name]
    $before = if (Test-Path -LiteralPath $path) { [IO.File]::ReadAllText($path) } else { '{}' }
    if ($name -eq 'codex') {
        # Codex is optional: a missing or broken CLI skips only Codex, never the whole installation.
        $candidates = @(CodexCandidates)
        $existing = $false
        foreach ($candidate in $candidates) {
            # Read-only preflight; the first CLI that lists the config as JSON is the real one.
            $run = InvokeCodex $candidate @('mcp','list','--json')
            if (-not $run.Ok -or $run.Output.Trim() -notmatch '^[\[{]') { continue }
            try { $entries = $run.Output | ConvertFrom-Json } catch { continue }
            $codexCommand = $candidate
            $existing = [bool]@($entries | Where-Object { $_ -and $_.PSObject.Properties['name'] -and $_.name -eq 'autogui' }).Count
            break
        }
        if (-not $codexCommand) {
            $tried = if ($candidates.Count) { 'tried: ' + ($candidates -join ', ') } else { 'none found on PATH or in the standard location' }
            Warn "Codex MCP not registered: no working codex CLI ($tried). If Codex is installed, its config.toml may be invalid. $codexHint"
        } elseif ($existing) {
            Warn 'Codex already has an autogui MCP server; it was left unchanged.'
        } else {
            [void]$changes.Add(@{Host=$name; Path=$path; Text=$null; BeforeHash=$(if (Test-Path -LiteralPath $path) { Hash $path } else { $null })})
        }
        continue
    } else {
        # Normalize only the parse input; the existing file is backed up as bytes.
        if ([string]::IsNullOrWhiteSpace($before)) { $before = '{}' }
        $definition = if ($name -eq 'opencode') { @{type='local';command=@($exe,'mcp');enabled=$true} } else { @{command=$exe;args=@('mcp')} }
        $container = if ($name -eq 'opencode') { 'mcp' } else { 'mcpServers' }
        $updated = [AutoGuiJson]::AddServer($before, $container, 'autogui', ($definition | ConvertTo-Json -Compress))
    }
    [void]$changes.Add(@{Host=$name; Path=$path; Text=$updated; BeforeHash=$(if (Test-Path -LiteralPath $path) { Hash $path } else { $null })})
}
Write-Host "Program: $InstallDir"
# The manifest lists only hosts this installation actually configures.
$McpHosts = @($McpHosts | Where-Object { $_ -ne 'codex' -or @($changes | Where-Object { $_.Host -eq 'codex' }).Count })
function SaveWarnings {
    if ($WarningFile -and $warnings.Count) { WriteUtf8 (Full $WarningFile) (($warnings.ToArray()) -join "`r`n") }
}
Write-Host "Skills: $($SkillHosts -join ', ')   MCP: $($McpHosts -join ', ')"
foreach ($change in $changes) { Write-Host "MCP config: $($change.Path)" }
if ($codexCommand) { Write-Host "Codex CLI: $codexCommand" }
if ($Preview) { SaveWarnings; Write-Host 'Preview only; no install/config writes.'; return }

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
        if ($change.Host -eq 'codex') {
            $run = InvokeCodex $codexCommand @('mcp','add','autogui','--',$exe,'mcp')
            $after = if (Test-Path -LiteralPath $change.Path) { Hash $change.Path } else { $null }
            if (-not $run.Ok -and $after -eq $current) {
                # Nothing was written: skip Codex only and keep the rest of the installation.
                if ($backup) { Remove-Item -LiteralPath $backup -Force }
                $McpHosts = @($McpHosts | Where-Object { $_ -ne 'codex' })
                Warn "Codex MCP not registered: '$codexCommand mcp add' failed. $codexHint"
                continue
            }
            if (-not $run.Ok) { throw 'Codex MCP registration failed after changing config.toml; rolling back.' }
        }
        try {
            if ($change.Host -ne 'codex') { WriteUtf8 $change.Path $change.Text }
        } finally {
            if (Test-Path -LiteralPath $change.Path) {
                [void]$records.Add(@{path=$change.Path;sha256=(Hash $change.Path);backup=$backup})
                SaveManifest
            }
        }
    }
    SaveManifest
    SaveWarnings
    Write-Host 'Installed. Restart selected agents to discover the Skill/MCP tools.'
    Write-Host "Run: & '$exe' --help"
    Write-Host "Uninstall: & '$InstallDir\uninstall.ps1'"
} catch {
    SaveManifest
    Write-Warning 'Installation failed. Rolling back owned files; changed files will be preserved.'
    & (Join-Path $package 'uninstall.ps1') -InstallDir $InstallDir
    throw
}
