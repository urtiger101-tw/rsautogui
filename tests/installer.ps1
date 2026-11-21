# Runs only against disposable profile directories in .agent; never the real profile.
[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$PackageRoot)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$fixture=Join-Path $repo ('.agent\install-test-'+[guid]::NewGuid().ToString('N'))
$profile=Join-Path $fixture 'profile'
$install=Join-Path $fixture 'program'
New-Item -ItemType Directory -Path $profile -Force | Out-Null
function Assert($Condition,[string]$Message) { if (-not $Condition) { throw $Message } }
$utf8=New-Object System.Text.UTF8Encoding($false)
$originals=@{
    '.claude.json' = '{"theme":"dark","mcpServers":{"fixture":{"command":"not-started"}}}'
    '.config\opencode\opencode.jsonc' = @'
{
  // preserve this comment and trailing commas
  "model": "fixture/unused",
  "mcp": { "fixture": {"type":"local","command":["unused"],}, },
}
'@
    '.gemini\config\mcp_config.json' = '{"mcpServers":{"fixture":{"command":"unused"}},"other":true}'
    '.codex\config.toml' = "# keep comment`nmodel = 'gpt-6-astra'`n"
}
foreach($relative in $originals.Keys) {
    $path=Join-Path $profile $relative
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    [IO.File]::WriteAllText($path,$originals[$relative],$utf8)
}
& (Join-Path $PackageRoot 'install.ps1') -HomeDirectory $profile -InstallDir $install -SkillHosts codex,claude,opencode,agy -McpHosts codex,claude,opencode,agy -NonInteractive -Preview
Assert (-not (Test-Path -LiteralPath $install)) 'Preview wrote installation files'
& (Join-Path $PackageRoot 'install.ps1') -HomeDirectory $profile -InstallDir $install -SkillHosts codex,claude,opencode,agy -McpHosts codex,claude,opencode,agy -NonInteractive
Assert (Test-Path -LiteralPath (Join-Path $install 'autogui_runtime.dll')) 'Runtime DLL missing'
foreach($relative in @('.codex\skills','.claude\skills','.config\opencode\skills','.gemini\config\skills')) {
    $root=Join-Path $profile "$relative\autogui-control"
    Assert (Test-Path -LiteralPath (Join-Path $root 'SKILL.md')) "Missing Skill $relative"
    $runtime=Get-Content -LiteralPath (Join-Path $root 'runtime.json') -Raw | ConvertFrom-Json
    Assert ($runtime.executable -eq (Join-Path $install 'autogui-control.exe')) 'Wrong Skill runtime path'
}
Assert (-not (Test-Path -LiteralPath (Join-Path $profile '.gemini\antigravity-cli\skills'))) 'Default AGY Skill used legacy root'
$claude=Get-Content -LiteralPath (Join-Path $profile '.claude.json') -Raw | ConvertFrom-Json
Assert ($claude.theme -eq 'dark' -and $claude.mcpServers.fixture.command -eq 'not-started') 'Unrelated Claude config lost'
Assert ($claude.mcpServers.autogui.args[0] -eq 'mcp') 'Claude MCP args wrong'
$agy=Get-Content -LiteralPath (Join-Path $profile '.gemini\config\mcp_config.json') -Raw | ConvertFrom-Json
Assert ($agy.other -and $agy.mcpServers.fixture.command -eq 'unused') 'Unrelated AGY config lost'
Assert ($agy.mcpServers.autogui.command -eq (Join-Path $install 'autogui-control.exe') -and $agy.mcpServers.autogui.args[0] -eq 'mcp') 'AGY MCP definition wrong'
$oc=[IO.File]::ReadAllText((Join-Path $profile '.config\opencode\opencode.jsonc'))
Assert ($oc.Contains('// preserve this comment and trailing commas')) 'JSONC comment lost'
Assert ($oc.Contains('"fixture": {"type":"local","command":["unused"],}')) 'JSONC formatting changed'
& (Join-Path $install 'autogui-control.exe') --help | Out-Null
Assert ($LASTEXITCODE -eq 0) 'Installed launcher failed'
& (Join-Path $install 'uninstall.ps1')
foreach($relative in $originals.Keys) {
    Assert ([IO.File]::ReadAllText((Join-Path $profile $relative)) -ceq $originals[$relative]) "Original not restored: $relative"
}
Assert (-not (Test-Path -LiteralPath (Join-Path $install 'autogui-control.exe'))) 'Program was not removed'
Assert (-not (Test-Path -LiteralPath (Join-Path $profile '.gemini\config\skills\autogui-control\SKILL.md'))) 'Shared AGY Skill was not removed'
& (Join-Path $PackageRoot 'install.ps1') -HomeDirectory $profile -InstallDir $install -NonInteractive
& (Join-Path $install 'uninstall.ps1')

# Program-only installation must not create any host roots.
$profile2=Join-Path $fixture 'profile-program-only'
$install2=Join-Path $fixture 'program-only'
& (Join-Path $PackageRoot 'install.ps1') -HomeDirectory $profile2 -InstallDir $install2 -NonInteractive
Assert (-not (Test-Path -LiteralPath $profile2)) 'Program-only installation wrote agent profile'
& (Join-Path $install2 'uninstall.ps1')

# Collision must fail before installing any program file.
$profile3=Join-Path $fixture 'profile-collision'
New-Item -ItemType Directory -Path (Join-Path $profile3 '.claude\skills\autogui-control') -Force | Out-Null
$failed=$false
try { & (Join-Path $PackageRoot 'install.ps1') -HomeDirectory $profile3 -InstallDir (Join-Path $fixture 'collision') -SkillHosts claude -NonInteractive } catch { $failed=$true }
Assert $failed 'Existing Skill not protected'
Assert (-not (Test-Path -LiteralPath (Join-Path $fixture 'collision'))) 'Collision left a partial installation'

# Preserve user edits to a newly created host config and unrelated Skill roots.
$profile4=Join-Path $fixture 'profile-edited'
$install4=Join-Path $fixture 'program-edited'
& (Join-Path $PackageRoot 'install.ps1') -HomeDirectory $profile4 -InstallDir $install4 -SkillHosts claude -McpHosts claude -NonInteractive
$editedPath=Join-Path $profile4 '.claude.json'
$editedText=[IO.File]::ReadAllText($editedPath).Replace('"mcpServers"','"userAdded": true, "mcpServers"')
[IO.File]::WriteAllText($editedPath,$editedText,$utf8)
& (Join-Path $install4 'uninstall.ps1')
Assert ([IO.File]::ReadAllText($editedPath) -ceq $editedText) 'User config edit overwritten during uninstall'
Assert (-not (Test-Path -LiteralPath (Join-Path $profile4 '.codex'))) 'Unselected host was modified'

# Empty and whitespace-only configs are objects for parsing, but backups and
# uninstall must retain the exact original bytes (including an encoding BOM).
foreach($case in @(
    @{Name='empty';Bytes=[byte[]]@()},
    @{Name='whitespace';Bytes=[byte[]](@(0xef,0xbb,0xbf)+$utf8.GetBytes(" `t`r`n"))}
)) {
    $blankProfile=Join-Path $fixture ("profile-"+$case.Name)
    $blankInstall=Join-Path $fixture ("program-"+$case.Name)
    $blankConfig=Join-Path $blankProfile '.gemini\config\mcp_config.json'
    New-Item -ItemType Directory -Path (Split-Path -Parent $blankConfig) -Force | Out-Null
    [IO.File]::WriteAllBytes($blankConfig,$case.Bytes)
    $originalBytes=[Convert]::ToBase64String($case.Bytes)
    $otherConfig=Join-Path $blankProfile '.gemini\config\settings.json'
    [IO.File]::WriteAllText($otherConfig,'{"theme":"fixture","permissions":"unchanged"}',$utf8)
    $otherText=[IO.File]::ReadAllText($otherConfig)
    & (Join-Path $PackageRoot 'install.ps1') -HomeDirectory $blankProfile -InstallDir $blankInstall -McpHosts agy -NonInteractive -Preview
    Assert (-not (Test-Path -LiteralPath $blankInstall)) "Preview wrote program files for $($case.Name) config"
    Assert ([Convert]::ToBase64String([IO.File]::ReadAllBytes($blankConfig)) -ceq $originalBytes) "Preview changed $($case.Name) config"
    & (Join-Path $PackageRoot 'install.ps1') -HomeDirectory $blankProfile -InstallDir $blankInstall -McpHosts agy -NonInteractive
    $blankParsed=Get-Content -LiteralPath $blankConfig -Raw | ConvertFrom-Json
    Assert ($null -ne $blankParsed -and $blankParsed.mcpServers.autogui.command -eq (Join-Path $blankInstall 'autogui-control.exe')) "$($case.Name) config was not installed as an object"
    $blankManifest=Get-Content -LiteralPath (Join-Path $blankInstall '.install\manifest.json') -Raw | ConvertFrom-Json
    $configRecord=@($blankManifest.files | Where-Object path -EQ $blankConfig)
    Assert ($configRecord.Count -eq 1 -and (Test-Path -LiteralPath $configRecord[0].backup)) "Original backup missing for $($case.Name) config"
    Assert ([Convert]::ToBase64String([IO.File]::ReadAllBytes($configRecord[0].backup)) -ceq $originalBytes) "Original backup bytes changed for $($case.Name) config"
    & (Join-Path $blankInstall 'uninstall.ps1')
    Assert (Test-Path -LiteralPath $blankConfig) "Uninstall removed existing $($case.Name) config"
    Assert ([Convert]::ToBase64String([IO.File]::ReadAllBytes($blankConfig)) -ceq $originalBytes) "Uninstall changed original $($case.Name) config bytes"
    Assert ([IO.File]::ReadAllText($otherConfig) -ceq $otherText) 'Unrelated AGY settings changed'
    $retainedBackups=@(Get-ChildItem -LiteralPath $fixture -Directory -Filter ((Split-Path -Leaf $blankInstall)+'.recovery-*') | ForEach-Object { Join-Path $_.FullName 'backups\agy.original' })
    Assert ($retainedBackups.Count -eq 1 -and (Test-Path -LiteralPath $retainedBackups[0])) "Recovery backup not retained for $($case.Name) config"
    Assert ([Convert]::ToBase64String([IO.File]::ReadAllBytes($retainedBackups[0])) -ceq $originalBytes) "Retained backup bytes changed for $($case.Name) config"
}

# Explicit roots support legacy CLI installs and custom shared locations.
# Uninstall follows recorded file paths and preserves other skills in each root.
foreach($case in @(
    @{Name='legacy';Root='.gemini\antigravity-cli\skills'},
    @{Name='custom';Root='custom shared skills'}
)) {
    $overrideProfile=Join-Path $fixture ("profile-override-"+$case.Name)
    $overrideInstall=Join-Path $fixture ("program-override-"+$case.Name)
    $overrideRoot=Join-Path $overrideProfile $case.Root
    $unrelatedSkill=Join-Path $overrideRoot 'other-skill\SKILL.md'
    New-Item -ItemType Directory -Path (Split-Path -Parent $unrelatedSkill) -Force | Out-Null
    [IO.File]::WriteAllText($unrelatedSkill,'fixture: preserve this skill',$utf8)
    & (Join-Path $PackageRoot 'install.ps1') -HomeDirectory $overrideProfile -InstallDir $overrideInstall -SkillHosts agy -AgySkillRoot $overrideRoot -NonInteractive
    $overrideSkill=Join-Path $overrideRoot 'autogui-control'
    Assert (Test-Path -LiteralPath (Join-Path $overrideSkill 'SKILL.md')) "AGY $($case.Name) Skill root ignored"
    $overrideRuntime=Get-Content -LiteralPath (Join-Path $overrideSkill 'runtime.json') -Raw | ConvertFrom-Json
    Assert ($overrideRuntime.executable -eq (Join-Path $overrideInstall 'autogui-control.exe')) 'Override Skill runtime path wrong'
    Assert (-not (Test-Path -LiteralPath (Join-Path $overrideProfile '.gemini\config'))) 'AGY Skill override wrote default root or MCP config'
    & (Join-Path $overrideInstall 'uninstall.ps1')
    Assert (-not (Test-Path -LiteralPath (Join-Path $overrideSkill 'SKILL.md'))) "AGY $($case.Name) Skill was not removed"
    Assert ([IO.File]::ReadAllText($unrelatedSkill) -ceq 'fixture: preserve this skill') 'Unrelated override Skill changed'
}
# Claude Desktop is MCP-only. Its config path derives from the isolated profile (AppData\Roaming),
# never from the real %APPDATA%; the fixture asserts that before any write.
$desktopProfile=Join-Path $fixture 'profile-claude-desktop'
$desktopInstall=Join-Path $fixture 'program-claude-desktop'
$desktopConfig=Join-Path $desktopProfile 'AppData\Roaming\Claude\claude_desktop_config.json'
Assert (-not ($env:APPDATA -and $desktopConfig.StartsWith($env:APPDATA,[StringComparison]::OrdinalIgnoreCase))) 'Claude Desktop fixture points at the real APPDATA'
$desktopOriginal='{"preferences":{"keep":true},"mcpServers":{"fixture":{"command":"unused"}}}'
New-Item -ItemType Directory -Path (Split-Path -Parent $desktopConfig) -Force | Out-Null
[IO.File]::WriteAllText($desktopConfig,$desktopOriginal,$utf8)
$failed=$false
try { &(Join-Path $PackageRoot 'install.ps1') -HomeDirectory $desktopProfile -InstallDir (Join-Path $fixture 'desktop-rejected') -SkillHosts claude-desktop -NonInteractive } catch { $failed=$true }
Assert $failed 'Claude Desktop accepted as a Skill host'
Assert (-not (Test-Path -LiteralPath (Join-Path $fixture 'desktop-rejected'))) 'Rejected Skill host left files'
&(Join-Path $PackageRoot 'install.ps1') -HomeDirectory $desktopProfile -InstallDir $desktopInstall -McpHosts claude-desktop -NonInteractive -Preview
Assert (-not (Test-Path -LiteralPath $desktopInstall)) 'Preview wrote Claude Desktop installation files'
Assert ([IO.File]::ReadAllText($desktopConfig) -ceq $desktopOriginal) 'Preview changed Claude Desktop config'
&(Join-Path $PackageRoot 'install.ps1') -HomeDirectory $desktopProfile -InstallDir $desktopInstall -McpHosts claude-desktop -NonInteractive
$desk=Get-Content -LiteralPath $desktopConfig -Raw | ConvertFrom-Json
Assert ($desk.preferences.keep -eq $true -and $desk.mcpServers.fixture.command -eq 'unused') 'Unrelated Claude Desktop config lost'
Assert ($desk.mcpServers.autogui.command -eq (Join-Path $desktopInstall 'autogui-control.exe') -and $desk.mcpServers.autogui.args[0] -eq 'mcp') 'Claude Desktop MCP definition wrong'
Assert (-not (Test-Path -LiteralPath (Join-Path $desktopProfile '.claude'))) 'Claude Desktop install wrote Claude Code roots'
&(Join-Path $desktopInstall 'uninstall.ps1')
Assert ([IO.File]::ReadAllText($desktopConfig) -ceq $desktopOriginal) 'Uninstall did not restore Claude Desktop config bytes'
Assert (-not (Test-Path -LiteralPath (Join-Path $desktopInstall 'autogui-control.exe'))) 'Claude Desktop uninstall left the program'
# A missing config is created by install and removed by uninstall.
$newProfile=Join-Path $fixture 'profile-claude-desktop-new'
$newInstall=Join-Path $fixture 'program-claude-desktop-new'
$newConfig=Join-Path $newProfile 'AppData\Roaming\Claude\claude_desktop_config.json'
&(Join-Path $PackageRoot 'install.ps1') -HomeDirectory $newProfile -InstallDir $newInstall -McpHosts claude-desktop -NonInteractive
Assert ((Get-Content -LiteralPath $newConfig -Raw | ConvertFrom-Json).mcpServers.autogui.args[0] -eq 'mcp') 'Missing Claude Desktop config was not created'
&(Join-Path $newInstall 'uninstall.ps1')
Assert (-not (Test-Path -LiteralPath $newConfig)) 'Uninstall left a Claude Desktop config it created'
Write-Host "PASS installer: all four hosts, claude-desktop MCP (isolated AppData, preview, byte-exact restore, created-config removal, Skill rejection), JSONC preservation, byte-exact empty/whitespace backups and restoration, shared/legacy/custom AGY Skill roots, program-only, collision, isolated root $fixture"
