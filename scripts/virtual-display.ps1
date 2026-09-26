#requires -Version 5.1
[CmdletBinding()]
param(
    [ValidateSet('Status','Prepare','Install','Remove')][string]$Action = 'Status',
    [string]$CacheDirectory = (Join-Path $env:LOCALAPPDATA 'rsautogui\virtual-display\25.7.23'),
    [ValidateRange(640,7680)][int]$Width = 1920,
    [ValidateRange(480,4320)][int]$Height = 1080,
    [ValidateRange(24,240)][int]$RefreshRate = 60
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2
$utf8 = New-Object Text.UTF8Encoding($false)
$windowsRoot = [IO.Path]::GetPathRoot([Environment]::GetFolderPath('Windows'))
$driverDirectory = Join-Path $windowsRoot 'VirtualDisplayDriver'
$statePath = Join-Path $env:ProgramData 'rsautogui\virtual-display.json'
$CacheDirectory = [IO.Path]::GetFullPath($CacheDirectory)
function Devices {
    @(Get-CimInstance Win32_PnPEntity -Filter "PNPClass = 'Display'" | Where-Object {
        @($_.HardwareID | Where-Object { $_ -in @('Root\MttVDD','MttVDD') }).Count -gt 0
    } | Select-Object @{n='InstanceId';e={$_.PNPDeviceID}},@{n='FriendlyName';e={$_.Name}},Status)
}
function SaveState($State) {
    [IO.Directory]::CreateDirectory((Split-Path -Parent $statePath)) | Out-Null
    [IO.File]::WriteAllText($statePath, ($State | ConvertTo-Json -Depth 8), $utf8)
}
function RequireAdmin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Driver changes require an Administrator PowerShell. Prepare and Status do not require elevation.'
    }
}
function Archive([string]$Name,[string]$Url,[string]$ExpectedHash) {
    [IO.Directory]::CreateDirectory($CacheDirectory) | Out-Null
    $path = Join-Path $CacheDirectory $Name
    if (-not (Test-Path -LiteralPath $path)) { Invoke-WebRequest -Uri $Url -OutFile $path -UseBasicParsing }
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $ExpectedHash) {
        throw "Downloaded archive hash mismatch; preserved for inspection: $path"
    }
    $path
}
function ExtractFile([string]$Archive,[string]$Entry,[string]$Destination) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($Archive)
    try {
        $item = $zip.GetEntry($Entry)
        if ($null -eq $item) { throw "Missing pinned archive entry: $Entry" }
        [IO.Directory]::CreateDirectory((Split-Path -Parent $Destination)) | Out-Null
        $inputStream = $item.Open()
        try {
            $buffer = New-Object IO.MemoryStream
            try {
                $inputStream.CopyTo($buffer)
                $bytes = $buffer.ToArray()
                if (Test-Path -LiteralPath $Destination) {
                    $sha = [Security.Cryptography.SHA256]::Create()
                    try { $expected = [BitConverter]::ToString($sha.ComputeHash($bytes)).Replace('-','') } finally { $sha.Dispose() }
                    if ((Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash -ne $expected) {
                        throw "Existing component changed; preserved: $Destination"
                    }
                } else { [IO.File]::WriteAllBytes($Destination, $bytes) }
            } finally { $buffer.Dispose() }
        } finally { $inputStream.Dispose() }
    } finally { $zip.Dispose() }
}
function CheckSignature([string]$Path,[string]$Publisher) {
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notlike $Publisher) {
        throw "Component signature is not trusted: $Path ($($signature.Status)). No certificate stores or signing policies were changed."
    }
}

if ([Environment]::OSVersion.Platform -ne 'Win32NT' -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString() -ne 'X64') {
    throw 'This optional driver package supports Windows x64 only.'
}
if ($Action -eq 'Status') {
    [pscustomobject]@{devices=@(Devices);state_file=$statePath;state_exists=(Test-Path -LiteralPath $statePath);driver_directory=$driverDirectory} | ConvertTo-Json -Depth 5
    return
}
if ($Action -eq 'Remove') {
    RequireAdmin
    if (-not (Test-Path -LiteralPath $statePath)) { throw 'No rsautogui-owned driver installation record; no devices were removed.' }
    $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
    $current = @(Devices)
    foreach ($id in @($state.device_ids)) {
        if (@($current | Where-Object InstanceId -eq $id).Count) {
            & "$env:SystemRoot\System32\pnputil.exe" /remove-device $id
            if ($LASTEXITCODE -notin @(0,3010)) { throw "Device removal failed: $LASTEXITCODE. Recovery record preserved." }
        }
    }
    $remaining = @(Devices | Where-Object { $_.InstanceId -in @($state.device_ids) })
    if ($remaining.Count) { throw 'Owned display device remains; a reboot or manual inspection may be required.' }
    $state.status = 'removed'
    SaveState $state
    Write-Output 'Removed owned virtual display devices. Driver-store package, configuration and recovery record are retained.'
    return
}
$previousState = $null
if ($Action -eq 'Install') {
    RequireAdmin
    if (@(Devices).Count) { throw 'An MttVDD driver already exists; preserved. Configure it with its original manager.' }
    if ((Test-Path -LiteralPath $driverDirectory) -or (Test-Path -LiteralPath $statePath)) {
        if (-not (Test-Path -LiteralPath $statePath)) { throw 'Existing driver directory was preserved; no ownership record.' }
        $previousState = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
        $existingProfile = Join-Path $driverDirectory 'vdd_settings.xml'
        if ($previousState.version -ne 1 -or $previousState.status -ne 'removed' -or
            $previousState.driver_directory -ne $driverDirectory -or
            -not (Test-Path -LiteralPath $existingProfile) -or
            (Get-FileHash -LiteralPath $existingProfile -Algorithm SHA256).Hash -ne $previousState.configuration_sha256) {
            throw 'Existing driver configuration or incomplete recovery record was preserved. Inspect it before installing again.'
        }
    }
}

$driverZip = Archive 'VirtualDisplayDriver-x86.Driver.Only.zip' 'https://github.com/VirtualDrivers/Virtual-Display-Driver/releases/download/25.7.23/VirtualDisplayDriver-x86.Driver.Only.zip' 'E24210692B442B39AF763536330CE78B423F19342B7A7792C26DE3944E418B3A'
$helperZip = Archive 'nefcon_v1.14.0.zip' 'https://github.com/nefarius/nefcon/releases/download/v1.14.0/nefcon_v1.14.0.zip' 'A15557DA24A9EFCA203158DE3B43B0EAF982DB231F0194031F1ED428BC13E669'
$prepared = Join-Path $CacheDirectory 'verified'
foreach ($file in @('MttVDD.inf','mttvdd.cat','MttVDD.dll')) {
    ExtractFile $driverZip "VirtualDisplayDriver/$file" (Join-Path $prepared $file)
}
$helper = Join-Path $prepared 'nefconc.exe'
ExtractFile $helperZip 'x64/nefconc.exe' $helper
CheckSignature (Join-Path $prepared 'mttvdd.cat') 'CN=SignPath Foundation,*'
CheckSignature $helper 'CN=Nefarius Software Solutions e.U.,*'
$configuration = @"
<?xml version="1.0" encoding="utf-8"?>
<vdd_settings>
  <monitors><count>1</count></monitors>
  <gpu><friendlyname>default</friendlyname></gpu>
  <global><g_refresh_rate>$RefreshRate</g_refresh_rate></global>
  <resolutions><resolution><width>$Width</width><height>$Height</height><refresh_rate>$RefreshRate</refresh_rate></resolution></resolutions>
  <options><CustomEdid>false</CustomEdid><HardwareCursor>true</HardwareCursor><SDR10bit>false</SDR10bit><HDRPlus>false</HDRPlus><logging>false</logging><debuglogging>false</debuglogging></options>
</vdd_settings>
"@
$profilePath = Join-Path $prepared "rsautogui-$Width-$Height-$RefreshRate.xml"
if ((Test-Path -LiteralPath $profilePath) -and [IO.File]::ReadAllText($profilePath) -cne $configuration) {
    throw "Existing prepared configuration changed; preserved: $profilePath"
}
[IO.File]::WriteAllText($profilePath, $configuration, $utf8)
if ($Action -eq 'Prepare') {
    [pscustomobject]@{status='prepared';driver_version='25.7.23';directory=$prepared;profile=$profilePath;monitors=1;width=$Width;height=$Height;refresh_rate=$RefreshRate;driver_catalog_signature='Valid';helper_signature='Valid';system_changes=$false} | ConvertTo-Json
    return
}

if ($null -ne $previousState) {
    $recovery = Join-Path (Split-Path -Parent $statePath) ('recovery\' + [Guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($recovery) | Out-Null
    Copy-Item -LiteralPath $statePath -Destination (Join-Path $recovery 'virtual-display.json')
    Copy-Item -LiteralPath (Join-Path $driverDirectory 'vdd_settings.xml') -Destination (Join-Path $recovery 'vdd_settings.xml')
}
$state = [pscustomobject]@{version=1;status='installing';created_at=(Get-Date -Format o);driver_directory=$driverDirectory;device_ids=@();width=$Width;height=$Height;refresh_rate=$RefreshRate;exit_code=$null;configuration_sha256=$null}
SaveState $state
[IO.Directory]::CreateDirectory($driverDirectory) | Out-Null
Copy-Item -LiteralPath $profilePath -Destination (Join-Path $driverDirectory 'vdd_settings.xml')
$state.configuration_sha256 = (Get-FileHash -LiteralPath (Join-Path $driverDirectory 'vdd_settings.xml') -Algorithm SHA256).Hash
SaveState $state
try {
    # Invoke only this pinned helper with this pinned INF and exact hardware ID.
    # Windows retains its driver trust checks; no cert import/test-signing changes.
    & $helper install (Join-Path $prepared 'MttVDD.inf') 'Root\MttVDD'
    $state.exit_code = $LASTEXITCODE
} finally {
    $devices = @(Devices)
    $state.device_ids = @($devices | ForEach-Object InstanceId)
    $state.status = 'needs-inspection'
    SaveState $state
}
if ($state.exit_code -notin @(0,3010)) { throw "Driver installation failed with exit $($state.exit_code). See $statePath; use Remove for owned devices. No trust policy was changed." }
if (-not $state.device_ids.Count) { throw "Installer returned without a visible device. Inspect $statePath before retrying." }
$state.status = if ($state.exit_code -eq 3010) { 'reboot-required' } elseif (@($devices | Where-Object Status -ne 'OK').Count) { 'needs-inspection' } else { 'installed' }
SaveState $state
$state | ConvertTo-Json -Depth 5
Write-Output 'Use autogui-control display list to confirm an active display. If needed, choose Extend in Windows Display Settings; keep the physical monitor primary. Capture is not verified by device installation alone.'
