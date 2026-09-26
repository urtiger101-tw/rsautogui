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
foreach($relative in @('.codex\skills','.claude\skills','.config\opencode\skills','.gemini\antigravity-cli\skills')) {
    $root=Join-Path $profile "$relative\autogui-control"
    Assert (Test-Path -LiteralPath (Join-Path $root 'SKILL.md')) "Missing Skill $relative"
    $runtime=Get-Content -LiteralPath (Join-Path $root 'runtime.json') -Raw | ConvertFrom-Json
    Assert ($runtime.executable -eq (Join-Path $install 'autogui-control.exe')) 'Wrong Skill runtime path'
}
$claude=Get-Content -LiteralPath (Join-Path $profile '.claude.json') -Raw | ConvertFrom-Json
Assert ($claude.theme -eq 'dark' -and $claude.mcpServers.fixture.command -eq 'not-started') 'Unrelated Claude config lost'
Assert ($claude.mcpServers.autogui.args[0] -eq 'mcp') 'Claude MCP args wrong'
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
Write-Host "PASS installer: all four hosts, JSONC preservation, config restoration, program-only, collision, isolated root $fixture"
