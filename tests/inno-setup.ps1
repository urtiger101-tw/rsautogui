# Compiles installer/autogui.iss with a throwaway AppId and installs only into
# disposable .agent fixtures with no agent-host tasks. It never uses the real
# profile or the default %LOCALAPPDATA%\Programs\AutoGui directory.
[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$PackageRoot, [string]$IsccPath)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
function Assert($Condition,[string]$Message) { if (-not $Condition) { throw $Message } }
function Find-Iscc {
    if ($IsccPath) { return $IsccPath }
    $candidates=@(
        $env:ISCC,
        $(if ($env:LOCALAPPDATA) { Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe' }),
        $(if (${env:ProgramFiles(x86)}) { Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe' }),
        $(if ($env:ProgramFiles) { Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe' })
    )
    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path -LiteralPath $candidate -PathType Leaf)) { return $candidate }
    }
    throw 'ISCC.exe not found; pass -IsccPath.'
}
function Run-Setup([string]$File,[string[]]$Options) {
    $arguments=(@('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART') + $Options) -join ' '
    return (Start-Process -FilePath $File -ArgumentList $arguments -Wait -PassThru).ExitCode
}
$PackageRoot=[IO.Path]::GetFullPath($PackageRoot)
Assert (Test-Path -LiteralPath (Join-Path $PackageRoot 'install.ps1')) 'PackageRoot lacks install.ps1'
Assert (Test-Path -LiteralPath (Join-Path $PackageRoot 'payload.json')) 'PackageRoot lacks payload.json'
$iscc=Find-Iscc
$fixture=Join-Path $repo ('.agent\inno-test-'+[guid]::NewGuid().ToString('N'))
$program=Join-Path $fixture 'program'
$appId=[guid]::NewGuid().ToString().ToUpperInvariant()
$version='0.0.0-test'
New-Item -ItemType Directory -Path $fixture -Force | Out-Null
& $iscc /Qp "/DAppVersion=$version" "/DAppIdGuid=$appId" "/DPackageDir=$PackageRoot" "/DOutputDir=$fixture" (Join-Path $repo 'installer\autogui.iss')
Assert ($LASTEXITCODE -eq 0) "ISCC failed with exit $LASTEXITCODE"
$setup=Join-Path $fixture "AutoGui-$version-setup.exe"
Assert (Test-Path -LiteralPath $setup) 'Setup executable was not produced'
$uninstaller="$program.setup\unins000.exe"
$registryKey="HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{$appId}_is1"

# Fresh install: no tasks, so install.ps1 receives no host lists.
$code=Run-Setup $setup @("/DIR=""$program""","/LOG=""$(Join-Path $fixture 'install-1.log')""")
Assert ($code -eq 0) "Install exited $code; see install-1.log"
Assert (Test-Path -LiteralPath (Join-Path $program 'autogui-control.exe')) 'Launcher not installed'
Assert (Test-Path -LiteralPath (Join-Path $program 'autogui_runtime.dll')) 'Runtime DLL not installed'
Assert (Test-Path -LiteralPath (Join-Path $program '.install\manifest.json')) 'Manifest not written by install.ps1'
Assert (-not (Test-Path -LiteralPath (Join-Path $program 'unins000.exe'))) 'Inno wrote into the program directory'
Assert (Test-Path -LiteralPath $uninstaller) 'Uninstaller not written beside the program directory'
Assert (Test-Path -LiteralPath $registryKey) 'Per-user uninstall registration missing'
& (Join-Path $program 'autogui-control.exe') --help | Out-Null
Assert ($LASTEXITCODE -eq 0) 'Installed launcher --help failed'

# Upgrade over the same AppId: Inno runs the old uninstaller, then install.ps1 reinstalls.
$code=Run-Setup $setup @("/DIR=""$program""","/LOG=""$(Join-Path $fixture 'install-2.log')""")
Assert ($code -eq 0) "Upgrade exited $code; see install-2.log"
Assert (Test-Path -LiteralPath (Join-Path $program 'autogui-control.exe')) 'Upgrade did not reinstall the launcher'
Assert (Test-Path -LiteralPath (Join-Path $program '.install\manifest.json')) 'Upgrade did not write a manifest'

# Uninstall removes manifest-owned files and retains recovery data as documented.
$code=Run-Setup $uninstaller @()
Assert ($code -eq 0) "Uninstall exited $code"
Assert (-not (Test-Path -LiteralPath (Join-Path $program 'autogui-control.exe'))) 'Uninstall left the launcher'
Assert (-not (Test-Path -LiteralPath (Join-Path $program '.install'))) 'Uninstall left the manifest directory'
Assert (-not (Test-Path -LiteralPath "$program.setup")) 'Uninstaller data remained'
Assert (-not (Test-Path -LiteralPath $registryKey)) 'Uninstall left the registration'
$recovery=@(Get-ChildItem -LiteralPath $fixture -Directory -Filter 'program.recovery-*')
Assert ($recovery.Count -ge 1) 'Recovery backups were not retained'

# Failure path: a pre-existing file makes install.ps1 refuse; Setup must exit non-zero
# and leave that file untouched.
$collision=Join-Path $fixture 'program-collision'
New-Item -ItemType Directory -Path $collision -Force | Out-Null
Set-Content -LiteralPath (Join-Path $collision 'user-file.txt') -Value 'keep' -Encoding ASCII
$code=Run-Setup $setup @("/DIR=""$collision""","/LOG=""$(Join-Path $fixture 'install-collision.log')""")
Assert ($code -ne 0) 'Collision install unexpectedly succeeded'
Assert (-not (Test-Path -LiteralPath (Join-Path $collision 'autogui-control.exe'))) 'Collision left program files'
Assert ((Get-Content -LiteralPath (Join-Path $collision 'user-file.txt') -Raw).Trim() -eq 'keep') 'Collision changed user file'
Write-Host "PASS inno-setup: test AppId $appId, fresh install, upgrade, uninstall, collision exit $code; isolated fixture $fixture"
