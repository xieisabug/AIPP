$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)

function Read-SearchProcessSnapshot {
    $all = @(Get-CimInstance Win32_Process -ErrorAction Stop)
    $alive = @{}
    foreach ($item in $all) { $alive[[uint32]$item.ProcessId] = $true }
    foreach ($item in $all) {
        if ($item.Name -ieq 'chrome.exe' -and $item.CommandLine) {
            [pscustomobject]@{
                pid = [uint32]$item.ProcessId
                parent_alive = $alive.ContainsKey([uint32]$item.ParentProcessId)
                name = [string]$item.Name
                command_line = [string]$item.CommandLine
                created = $item.CreationDate.ToUniversalTime().Ticks.ToString()
            }
        }
    }
}

if ($env:AIPP_SEARCH_EXPECTED_PROCESS) {
    $expected = $env:AIPP_SEARCH_EXPECTED_PROCESS | ConvertFrom-Json
    $process = Get-Process -Id $expected.pid -ErrorAction SilentlyContinue
    if ($null -eq $process) { Write-Output '[]'; exit 0 }
    try {
        # Hold an OS handle before revalidating, so PID reuse cannot change the kill target.
        $null = $process.Handle
        $current = @(Read-SearchProcessSnapshot | Where-Object { $_.pid -eq $expected.pid })
        if ($current.Count -ne 1 -or $current[0].created -cne $expected.created -or
            $current[0].command_line -cne $expected.command_line -or $current[0].parent_alive) {
            throw "Process identity or parent state changed; refusing to stop pid=$($expected.pid)"
        }
        if (-not $process.HasExited) {
            $process.Kill()
            if (-not $process.WaitForExit(3000)) { throw "Chrome pid=$($expected.pid) did not exit within 3000ms" }
        }
    } finally { $process.Dispose() }
    Write-Output '[]'
} else {
    $snapshot = @(Read-SearchProcessSnapshot)
    ConvertTo-Json -InputObject $snapshot -Compress -Depth 4
}
