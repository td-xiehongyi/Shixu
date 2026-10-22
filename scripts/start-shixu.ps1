param([switch]$CheckOnly, [ValidateSet('Framework','Shell')][string]$Mode = 'Framework')
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskAppDir = Join-Path $taskRoot 'target\x86_64-pc-windows-msvc\debug'
$taskApp = Join-Path $taskAppDir 'shixu-desktop.exe'
$taskWorkDir = Join-Path $taskRoot 'tools\NapCat.Shell'
$taskNapCatDir = Join-Path $taskRoot ('tools\NapCat.' + $Mode)
. (Join-Path $PSScriptRoot 'unified-start-plan.ps1')
try {
    $taskRequired = @($taskApp, (Join-Path $taskAppDir 'vault-win-x64\runtime\node.exe'),
        (Join-Path $taskAppDir 'vault-win-x64\helper\helper.mjs'), (Join-Path $taskNapCatDir 'napcat.mjs'))
    if ($Mode -eq 'Framework') {
        $taskRequired += @((Join-Path $taskNapCatDir 'napimain.exe'), (Join-Path $taskNapCatDir 'napiloader.dll'), (Join-Path $taskNapCatDir 'nativeLoader.cjs'))
        if (-not (Test-Path -LiteralPath $taskWorkDir -PathType Container)) { throw "Missing saved NapCat configuration directory: $taskWorkDir" }
    } else {
        $taskRequired += @((Join-Path $taskNapCatDir 'launcher-user.bat'), (Join-Path $taskNapCatDir 'NapCatWinBootMain.exe'))
    }
    foreach ($taskPath in $taskRequired) {
        if (-not (Test-Path -LiteralPath $taskPath -PathType Leaf)) { throw "Missing runtime file: $taskPath" }
    }
    if ($CheckOnly) { Write-Output "PASS: Shixu and project-local NapCat $Mode runtime paths verified; nothing launched."; exit 0 }
    $taskMutex = New-Object System.Threading.Mutex($false, 'Local\Shixu.UnifiedLauncher')
    $taskOwned = $false
    try {
        try { $taskOwned = $taskMutex.WaitOne(0) } catch [System.Threading.AbandonedMutexException] { $taskOwned = $true }
        if (-not $taskOwned) { Write-Output 'Shixu launch is already in progress.'; exit 0 }
        $taskProcesses = @(Get-Process -Name QQ,NapCatWinBootMain -ErrorAction SilentlyContinue)
        $taskNapCatRunning = Test-ShixuNapCatRunning -Mode $Mode -RuntimeDir $taskNapCatDir -Processes $taskProcesses
        $taskQQRunning = @($taskProcesses | Where-Object { $_.Name -eq 'QQ' }).Count -gt 0
        $taskAppRunning = @(Get-Process -Name shixu-desktop -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $taskApp }).Count -gt 0
        $taskPlan = Get-ShixuLaunchPlan -NapCatRunning $taskNapCatRunning -QQRunning $taskQQRunning -AppRunning $taskAppRunning
        if ($taskPlan.Blocked) { throw "QQ is already running without this project's NapCat $Mode. Exit QQ and any old NapCat normally, then launch Shixu again." }
        if ($taskPlan.StartNapCat) {
            if ($Mode -eq 'Framework') {
                $taskUninstall = (Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\QQ' -Name UninstallString).UninstallString
                $taskQQPath = Join-Path (Split-Path -Parent $taskUninstall.Trim('"')) 'QQ.exe'
                if (-not (Test-Path -LiteralPath $taskQQPath -PathType Leaf)) { throw 'Registered QQ installation is missing.' }
                # Use the release's native hook; leave QQ package.json and its GUI entry intact.
                # Reuse saved config in Shell's work directory, without copying or reading tokens.
                $taskEnvNames = @('NAPCAT_WORKDIR','NAPCAT_MAIN_PATH','NAPCAT_PATCH_PACKAGE','NAPCAT_LOAD_PATH')
                $taskOldEnv = @{}
                foreach ($taskName in $taskEnvNames) { $taskOldEnv[$taskName] = [Environment]::GetEnvironmentVariable($taskName,'Process') }
                try {
                    $env:NAPCAT_WORKDIR = $taskWorkDir
                    $env:NAPCAT_MAIN_PATH = (Join-Path $taskNapCatDir 'nativeLoader.cjs').Replace('\','/')
                    Remove-Item Env:NAPCAT_PATCH_PACKAGE,Env:NAPCAT_LOAD_PATH -ErrorAction SilentlyContinue
                    $taskArgs = @($taskQQPath, (Join-Path $taskNapCatDir 'napiloader.dll'), $env:NAPCAT_MAIN_PATH) | ForEach-Object { '"{0}"' -f $_ }
                    $taskNativeLogDir = Join-Path $taskRoot 'tmp\unified-start'
                    New-Item -ItemType Directory -Path $taskNativeLogDir -Force | Out-Null
                    $taskNative = Start-Process -FilePath (Join-Path $taskNapCatDir 'napimain.exe') -ArgumentList $taskArgs -WorkingDirectory $taskNapCatDir -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $taskNativeLogDir 'framework-stdout.log') -RedirectStandardError (Join-Path $taskNativeLogDir 'framework-stderr.log')
                } finally {
                    foreach ($taskName in $taskEnvNames) { [Environment]::SetEnvironmentVariable($taskName,$taskOldEnv[$taskName],'Process') }
                }
                # Hold the launch mutex until QQ has the hook, avoiding a second cold start.
                $taskHookDeadline = [DateTime]::UtcNow.AddSeconds(15)
                do {
                    $taskHookReady = Test-ShixuNapCatRunning -Mode Framework -RuntimeDir $taskNapCatDir -Processes @(Get-Process -Name QQ -ErrorAction SilentlyContinue)
                    if ($taskHookReady) { break }
                    if ($taskNative.HasExited -and $taskNative.ExitCode -ne 0) { throw 'Framework native launcher failed; see tmp/unified-start/framework-stderr.log.' }
                    Start-Sleep -Milliseconds 250
                } while ([DateTime]::UtcNow -lt $taskHookDeadline)
                if (-not $taskHookReady) { throw 'Framework hook was not found in QQ within 15 seconds; see tmp/unified-start/framework-stdout.log. Shixu was not started.' }
                Write-Output 'NapCat Framework hook loaded in QQ. Log in through the QQ window if requested.'
            } else {
                # Explicit fallback retains Shell's interactive login console.
                Start-Process -FilePath $env:ComSpec -ArgumentList @('/d','/c',('""{0}""' -f (Join-Path $taskNapCatDir 'launcher-user.bat'))) -WorkingDirectory $taskNapCatDir -WindowStyle Normal
                Write-Output 'NapCat Shell fallback started; this mode has no QQ chat window.'
            }
        } else { Write-Output "Existing project NapCat $Mode reused." }
        if ($taskPlan.StartApp) {
            $taskLogDir = Join-Path $taskRoot 'tmp\unified-start'
            New-Item -ItemType Directory -Path $taskLogDir -Force | Out-Null
            Start-Process -FilePath $taskApp -ArgumentList '--qq-auto-connect' -WorkingDirectory $taskAppDir -WindowStyle Normal -RedirectStandardError (Join-Path $taskLogDir 'shixu-stderr.log') -RedirectStandardOutput (Join-Path $taskLogDir 'shixu-stdout.log')
            Write-Output 'Shixu started. It will wait up to 5 minutes for QQ login and connect the saved source.'
        } else { Write-Output 'Shixu is already running. Connection status is available in Settings.' }
        Write-Output 'Credentials remain in Shixu protected storage. Details: Settings > notification source.'
    } finally {
        if ($taskOwned) { $taskMutex.ReleaseMutex() }
        $taskMutex.Dispose()
    }
} catch {
    Write-Error $_ -ErrorAction Continue
    exit 1
}
