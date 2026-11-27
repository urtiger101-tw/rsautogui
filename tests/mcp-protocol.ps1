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
    # app_launch must stay disabled by default regardless of the caller's environment.
    [void]$start.EnvironmentVariables.Remove('AUTOGUI_ALLOW_LAUNCH')
    # Tests never show the on-screen notice or write the user's action log.
    $start.EnvironmentVariables['AUTOGUI_OVERLAY']='0'
    $start.EnvironmentVariables['AUTOGUI_ACTION_LOG']='0'
    $start.StandardOutputEncoding=[Text.Encoding]::UTF8
    $start.StandardErrorEncoding=[Text.Encoding]::UTF8
    $process=New-Object Diagnostics.Process
    $process.StartInfo=$start
    [void]$process.Start()
    $stderr=$process.StandardError.ReadToEndAsync()
    @{Process=$process;ErrorTask=$stderr;Next=0;Notifications=@();Last=$null}
}
function Send($Client,$Message) { $Client.Process.StandardInput.WriteLine(($Message | ConvertTo-Json -Depth 15 -Compress));$Client.Process.StandardInput.Flush() }
function Receive($Client) {
    while ($true) {
        $read=$Client.Process.StandardOutput.ReadLineAsync()
        Assert ($read.Wait(15000)) 'MCP response timeout'
        $line=$read.Result
        Assert ($null -ne $line) 'MCP closed unexpectedly'
        $message=$line | ConvertFrom-Json
        # Servers may interleave notifications (e.g. notifications/message); clients skip them.
        if ($null -ne $message.method -and $null -eq $message.id) { $Client.Notifications += ,$message; continue }
        $Client.Last=$line
        return $message
    }
}
function Request($Client,[string]$Method,$Params) {
    $Client.Next++
    Send $Client @{jsonrpc='2.0';id=$Client.Next;method=$Method;params=$Params}
    $response=Receive $Client
    Assert ($response.id -eq $Client.Next) "Unexpected response id for $Method (stdout contamination?): $($Client.Last)"
    $response
}
function Initialize($Client,[string]$Version='2025-11-25') {
    $response=Request $Client initialize @{protocolVersion=$Version;capabilities=@{};clientInfo=@{name='isolated-fixture';version='1'}}
    Assert ($response.result.protocolVersion -eq $Version) 'Initialization version wrong'
    Send $Client @{jsonrpc='2.0';method='notifications/initialized'}
    $response.result
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
    $init=Initialize $client
    Assert ($init.serverInfo.name -eq 'autogui-control') 'serverInfo.name wrong'
    Assert ($init.serverInfo.title -eq 'AutoGui Windows Control') 'serverInfo.title missing'
    Assert ($null -ne $init.capabilities.logging) 'logging capability missing'
    Assert ($init.capabilities.tools.listChanged -eq $false) 'tools.listChanged must be false'
    $list=Request $client 'tools/list' @{}
    Assert ($list.result.tools.Count -eq 27) 'Wrong tool count'
    foreach ($tool in $list.result.tools) {
        Assert ($tool.title -is [string] -and $tool.title.Length -gt 0) "Tool $($tool.name) lacks title"
        Assert ($tool.inputSchema.type -eq 'object') "Tool $($tool.name) inputSchema not an object"
        Assert ($tool.outputSchema.type -eq 'object') "Tool $($tool.name) lacks outputSchema"
    }
    Assert ($null -ne (Request $client 'logging/setLevel' @{level='warning'}).result) 'logging/setLevel failed'
    Assert ((Request $client 'resources/list' @{}).result.resources.Count -eq 0) 'resources/list should be empty'
    Assert ((Request $client 'prompts/list' @{}).result.prompts.Count -eq 0) 'prompts/list should be empty'
    Assert (-not (Loaded $client)) 'DLL loaded just to initialize/list tools'
    $client.Process.StandardInput.WriteLine('{')
    $client.Process.StandardInput.Flush()
    Assert ((Receive $client).error.code -eq -32700) 'Malformed JSON not rejected'
    Send $client @{jsonrpc='2.0';method='notifications/unknown'}
    Assert ($null -ne (Request $client ping @{}).result) 'Notification polluted responses'
    Assert ((Request $client 'tools/call' @{name='shell';arguments=@{}}).error.code -eq -32602) 'Unknown tool accepted'
    $invalid=Tool $client type_text @{target='never-existed';text='unused';extra=$true}
    Assert $invalid.isError 'Unknown fields accepted'
    Assert ($invalid.structuredContent.code -eq 'invalid_argument') 'Tool error lacks structured code'
    Assert ($invalid.structuredContent.retryable -eq $false) 'Tool error lacks retryable'
    Assert (Loaded $client) 'First tool call did not load runtime DLL'
    foreach ($name in @('click_at','scroll','pointer_move','drag')) {
        $arguments=@{target='never-existed';capture_id='never-existed';x=1;y=1}
        if($name -eq 'scroll'){$arguments.clicks=-3}
        if($name -eq 'drag'){$arguments.to_x=2;$arguments.to_y=2}
        $withoutCapture=Tool $client $name $arguments
        Assert ($withoutCapture.isError -and $withoutCapture.content[0].text -match 'capture') 'Pointer input accepted without a capture proof'
    }
    $launch=Tool $client app_launch @{path='C:\Windows\System32\notepad.exe'}
    Assert ($launch.isError -and $launch.content[0].text -match 'AUTOGUI_ALLOW_LAUNCH' -and $launch.content[0].text -match 'Hint:') 'app_launch not disabled by default or missing recovery hint'
    Assert ($launch.structuredContent.code -eq 'permission_denied' -and $launch.structuredContent.hint -match 'AUTOGUI_ALLOW_LAUNCH') 'app_launch structured error wrong'
    $badKey=Tool $client hotkey @{target='never-existed';keys=@('notakey')}
    Assert ($badKey.isError -and $badKey.content[0].text -match 'unknown key') 'Unknown hotkey name not rejected before focus'
    $status=Tool $client screensaver @{action='status'}
    Assert (-not $status.isError) 'Screensaver status failed through DLL'
    Assert ($status.structuredContent.status_text -eq ($status.content[0].text | ConvertFrom-Json).status_text) 'structuredContent differs from JSON text'
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
        [void](Initialize $client)
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
    [void](Initialize $client)
    Assert ((Request $client 'tools/list' @{}).result.tools.Count -eq 27) 'Tool listing requires DLL'
    $missing=Tool $client screensaver @{action='status'}
    Assert ($missing.isError -and $missing.content[0].text -match 'autogui_runtime.dll') 'Missing DLL failure unclear'
    Assert ($missing.structuredContent.code -eq 'runtime_unavailable') 'Missing DLL error code wrong'
} finally {StopMcp $client}

# Older protocol versions negotiate and receive no structuredContent.
$client=StartMcp $separated
try {
    [void](Initialize $client '2025-03-26')
    $legacy=Tool $client screensaver @{action='status'}
    Assert ($legacy.isError -and $null -eq $legacy.structuredContent) 'structuredContent sent to a 2025-03-26 client'
} finally {StopMcp $client}
Write-Host "PASS MCP: lazy module list verified, JSON-RPC errors/notifications, tool schema (title/outputSchema), serverInfo.title, structured errors, version-gated structuredContent, DLL invocation, missing-DLL diagnostics; live screensaver=$LiveScreensaver"
