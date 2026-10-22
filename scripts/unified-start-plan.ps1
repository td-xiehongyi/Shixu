function Get-ShixuLaunchPlan {
    param([bool]$NapCatRunning, [bool]$QQRunning, [bool]$AppRunning)
    $blocked = $QQRunning -and -not $NapCatRunning
    [pscustomobject]@{
        Blocked = $blocked
        StartNapCat = -not $blocked -and -not $NapCatRunning
        StartApp = -not $blocked -and -not $AppRunning
    }
}

function Test-ShixuNapCatRunning {
    param([ValidateSet('Framework','Shell')][string]$Mode, [string]$RuntimeDir, [object[]]$Processes)
    foreach ($taskProcess in $Processes) {
        try {
            if ($Mode -eq 'Shell') {
                if ($taskProcess.Name -eq 'NapCatWinBootMain' -and $taskProcess.Path -ieq (Join-Path $RuntimeDir 'NapCatWinBootMain.exe')) { return $true }
            } elseif ($taskProcess.Name -eq 'QQ') {
                # The native launcher can exit while the injected GUI QQ remains running.
                foreach ($taskModule in $taskProcess.Modules) {
                    if ($taskModule.FileName -ieq (Join-Path $RuntimeDir 'napiloader.dll')) { return $true }
                }
            }
        } catch { # An inaccessible process is never assumed to be our receiver.
        }
    }
    return $false
}
