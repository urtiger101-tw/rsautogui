#requires -Version 5.1
[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$Executable,[switch]$LiveScreensaver)
$ErrorActionPreference='Stop'
$Executable=[IO.Path]::GetFullPath($Executable)
function Assert($Condition,[string]$Message) {if(-not $Condition){throw $Message}}
function StartMcp([string]$Path) {
    $start=New-Object Diagnostics.ProcessStartInfo
    $start.FileName=$Path
    $start.Arguments='mcp'
    $start.UseShellExecute=$false
    $start.CreateNoWindow=$true
    $start.RedirectStandardInput=$true
    $start.RedirectStandardOutput=$true
    $start.RedirectStandardError=$true
    $start.StandardOutputEncoding=[Text.Encoding]::UTF8
    $start.StandardErrorEncoding=[Text.Encoding]::UTF8
    $process=New-Object Diagnostics.Process
    $process.StartInfo=$start
    [void]$process.Start()
    $stderr=$process.StandardError.ReadToEndAsync()
    @{Process=$process;ErrorTask=$stderr;Next=0}
}
function Send($Client,$Message) { $Client.Process.StandardInput.WriteLine(($Message | ConvertTo-Json -Depth 15 -Compress));$Client.Process.StandardInput.Flush() }
function Receive($Client) {
    $read=$Client.Process.StandardOutput.ReadLineAsync()
    Assert ($read.Wait(15000)) 'MCP response timeout'
    $line=$read.Result
    Assert ($null -ne $line) 'MCP closed unexpectedly'
    $line | ConvertFrom-Json
}
function Request($Client,[string]$Method,$Params) {
    $Client.Next++
    Send $Client @{jsonrpc='2.0';id=$Client.Next;method=$Method;params=$Params}
    $response=Receive $Client
    Assert ($response.id -eq $Client.Next) 'Unexpected response id (stdout contamination?)'
    $response
}
function Initialize($Client) {
    $response=Request $Client initialize @{protocolVersion='2025-11-25';capabilities=@{};clientInfo=@{name='isolated-fixture';version='1'}}
    Assert ($response.result.protocolVersion -eq '2025-11-25') 'Initialization version wrong'
    Send $Client @{jsonrpc='2.0';method='notifications/initialized'}
}
function StopMcp($Client) {
    if(-not $Client.Process.HasExited){$Client.Process.StandardInput.Close(); if(-not $Client.Process.WaitForExit(10000)){$Client.Process.Kill();throw 'Server failed EOF shutdown'}}
    Assert ($Client.Process.ExitCode -eq 0) "MCP failed: $($Client.ErrorTask.Result)"
}
function Loaded($Client) { $Client.Process.Refresh(); @($Client.Process.Modules | Where-Object {$_.ModuleName -eq 'autogui_runtime.dll'}).Count -gt 0 }
function Tool($Client,[string]$Name,$Arguments) {
    $response=Request $Client 'tools/call' @{name=$Name;arguments=$Arguments}
    Assert ($null -eq $response.error) "Protocol error: $($response.error)"
    $response.result
}
$client=StartMcp $Executable
try {
    Assert (-not (Loaded $client)) 'DLL loaded on launch'
    $before=Request $client 'tools/list' @{}
    Assert ($before.error.code -eq -32002) 'Pre-initialize request should fail'
    Initialize $client
    $list=Request $client 'tools/list' @{}
    Assert ($list.result.tools.Count -eq 11) 'Wrong tool count'
    Assert (-not (Loaded $client)) 'DLL loaded just to initialize/list tools'
    $client.Process.StandardInput.WriteLine('{')
    $client.Process.StandardInput.Flush()
    Assert ((Receive $client).error.code -eq -32700) 'Malformed JSON not rejected'
    Send $client @{jsonrpc='2.0';method='notifications/unknown'}
    Assert ($null -ne (Request $client ping @{}).result) 'Notification polluted responses'
    Assert ((Request $client 'tools/call' @{name='shell';arguments=@{}}).error.code -eq -32602) 'Unknown tool accepted'
    $invalid=Tool $client type_text @{target='never-existed';text='unused';extra=$true}
    Assert $invalid.isError 'Unknown fields accepted'
    Assert (Loaded $client) 'First tool call did not load runtime DLL'
    $status=Tool $client screensaver @{action='status'}
    Assert (-not $status.isError) 'Screensaver status failed through DLL'
    if($LiveScreensaver) {
        $initial=$status.content[0].text | ConvertFrom-Json
        $pause=Tool $client screensaver @{action='pause';seconds=20}
        Assert (-not $pause.isError) "Managed pause failed: $($pause.content[0].text)"
        $during=Tool $client screensaver @{action='status'}
        $duringText=($during.content[0].text | ConvertFrom-Json).status_text
        Assert ($duringText -match '螢幕保護程式啟用：false') 'Pause did not disable auto-start'
        Assert ($duringText -match '本工具暫停：執行中') 'Managed pause is not active'
        StopMcp $client
        $client=StartMcp $Executable
        Initialize $client
        $after=Tool $client screensaver @{action='status'}
        $afterText=($after.content[0].text | ConvertFrom-Json).status_text
        Assert ($afterText -ceq $initial.status_text) 'MCP EOF did not restore exact screensaver status'
    }
} finally {StopMcp $client}

# A separated EXE remains able to describe tools and reports missing DLL clearly.
$fixture=Join-Path (Split-Path -Parent $PSScriptRoot) ('.agent\missing-dll-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixture | Out-Null
$separated=Join-Path $fixture 'autogui-control.exe'
Copy-Item -LiteralPath $Executable -Destination $separated
$client=StartMcp $separated
try {
    Initialize $client
    Assert ((Request $client 'tools/list' @{}).result.tools.Count -eq 11) 'Tool listing requires DLL'
    $missing=Tool $client screensaver @{action='status'}
    Assert ($missing.isError -and $missing.content[0].text -match 'autogui_runtime.dll') 'Missing DLL failure unclear'
} finally {StopMcp $client}
Write-Host "PASS MCP: lazy module list verified, JSON-RPC errors/notifications, tool schema, DLL invocation, missing-DLL diagnostics; live screensaver=$LiveScreensaver"
