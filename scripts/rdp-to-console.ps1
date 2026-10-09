#requires -Version 5.1
<#
Move the current Remote Desktop session to the physical/virtual console so the
desktop keeps rendering after the RDP client disconnects. The session stays
UNLOCKED on the console: anyone with physical access to that PC can use it.
Requires an Administrator PowerShell started inside the RDP session itself.
#>
[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2

$session = (Get-Process -Id $PID).SessionId
if ($session -eq 0) { throw 'Session 0 has no interactive desktop; run inside the logged-on RDP session.' }
if ($env:SESSIONNAME -eq 'Console') {
    Write-Output "Session $session is already on the console; nothing to do."
    return
}
if ($env:SESSIONNAME -notlike 'RDP-*') {
    throw "Session $session ($env:SESSIONNAME) is not a Remote Desktop session; nothing was changed."
}
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if (-not (New-Object Security.Principal.WindowsPrincipal($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'tscon to the console requires an Administrator PowerShell; nothing was changed.'
}
# 32-bit PowerShell on 64-bit Windows must bypass System32 redirection.
$system = if ([Environment]::Is64BitOperatingSystem -and -not [Environment]::Is64BitProcess) { 'Sysnative' } else { 'System32' }
$tscon = Join-Path $env:SystemRoot "$system\tscon.exe"
if (-not (Test-Path -LiteralPath $tscon)) { throw "tscon.exe is unavailable on this Windows edition: $tscon" }

Write-Warning 'The RDP client will disconnect and this desktop will stay signed in and unlocked on the console.'
Write-Warning 'Without a monitor, keep the optional virtual display installed (scripts/virtual-display.ps1) or a dummy plug attached so Windows has an active display.'
if ($PSCmdlet.ShouldProcess("session $session", 'tscon /dest:console')) {
    & $tscon $session /dest:console
    if ($LASTEXITCODE -ne 0) { throw "tscon failed with exit $LASTEXITCODE; the session was not moved." }
}
