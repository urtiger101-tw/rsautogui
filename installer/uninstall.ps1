#requires -Version 5.1
[CmdletBinding()]
param([string]$InstallDir = $PSScriptRoot)
$ErrorActionPreference = 'Stop'
$InstallDir = [IO.Path]::GetFullPath($InstallDir)
$manifestPath = Join-Path $InstallDir '.install\manifest.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
if ($manifest.version -ne 1 -or [IO.Path]::GetFullPath($manifest.install_dir) -ne $InstallDir) { throw 'Manifest does not match this installation.' }
$pending = New-Object System.Collections.ArrayList
$directories = New-Object System.Collections.ArrayList
$records = @($manifest.files)
[array]::Reverse($records)
foreach ($record in $records) {
    $path = [IO.Path]::GetFullPath($record.path)
    if (-not (Test-Path -LiteralPath $path)) { continue }
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $record.sha256) {
        Write-Warning "Modified file preserved; remove only AutoGui's entry manually if desired: $path"
        [void]$pending.Add($record)
        continue
    }
    if ($record.backup) {
        $backup = [IO.Path]::GetFullPath($record.backup)
        $allowed = (Join-Path $InstallDir '.install\backups').TrimEnd('\') + '\'
        if (-not $backup.StartsWith($allowed, [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid backup path' }
        Copy-Item -LiteralPath $backup -Destination $path -Force
    } else {
        Remove-Item -LiteralPath $path -Force
        [void]$directories.Add((Split-Path -Parent $path))
    }
}
# Only remove empty directories recorded by this installation, never recursively.
foreach ($directory in @($directories | Sort-Object Length -Descending -Unique)) {
    if ((Test-Path -LiteralPath $directory) -and @(Get-ChildItem -LiteralPath $directory -Force).Count -eq 0) { Remove-Item -LiteralPath $directory }
}
if ($pending.Count) {
    $manifest.files = @($pending.ToArray())
    [IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding($false)))
    Write-Warning "Partial uninstall: modified files and recovery metadata retained at $InstallDir"
} else {
    # Move only verified metadata to a unique sibling, keeping configs without
    # blocking reinstall into the same program directory.
    $source = [IO.Path]::GetFullPath((Join-Path $InstallDir '.install'))
    $parent = [IO.Path]::GetFullPath((Split-Path -Parent $InstallDir)).TrimEnd('\') + '\'
    $recovery = [IO.Path]::GetFullPath($InstallDir + '.recovery-' + [guid]::NewGuid().ToString('N'))
    if ($source -ne (Join-Path $InstallDir '.install') -or -not $source.StartsWith($parent, [StringComparison]::OrdinalIgnoreCase) -or -not $recovery.StartsWith($parent, [StringComparison]::OrdinalIgnoreCase)) { throw 'Recovery move is outside the installation parent.' }
    Move-Item -LiteralPath $source -Destination $recovery
    Write-Host "Uninstalled. Recovery backups retained: $recovery"
}
