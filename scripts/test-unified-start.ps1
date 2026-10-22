$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'unified-start-plan.ps1')
foreach ($case in @(
    @{ NapCat=$false; QQ=$false; App=$false; StartNapCat=$true; StartApp=$true; Blocked=$false },
    @{ NapCat=$true; QQ=$true; App=$false; StartNapCat=$false; StartApp=$true; Blocked=$false },
    @{ NapCat=$true; QQ=$true; App=$true; StartNapCat=$false; StartApp=$false; Blocked=$false },
    @{ NapCat=$false; QQ=$true; App=$false; StartNapCat=$false; StartApp=$false; Blocked=$true }
)) {
    $plan = Get-ShixuLaunchPlan -NapCatRunning $case.NapCat -QQRunning $case.QQ -AppRunning $case.App
    foreach ($key in @('StartNapCat','StartApp','Blocked')) {
        if ($plan.$key -ne $case.$key) { throw "Launch planning failed: $key" }
    }
}
Write-Output 'PASS: cold start, existing NapCat, duplicate start, ordinary QQ conflict'

# Exercise the real entry point in a disposable directory, never start a process.
$taskFixture = Join-Path ([IO.Path]::GetTempPath()) ('shixu launch check ' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path (Join-Path $taskFixture 'scripts') -Force | Out-Null
foreach ($taskScript in @('start-shixu.ps1','unified-start-plan.ps1')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot $taskScript) -Destination (Join-Path $taskFixture 'scripts')
}
$taskEntry = Get-ChildItem -LiteralPath (Split-Path $PSScriptRoot) -Filter '*.cmd' | Where-Object {
    [IO.File]::ReadAllText($_.FullName).Contains('scripts\start-shixu.ps1')
} | Select-Object -First 1
Copy-Item -LiteralPath $taskEntry.FullName -Destination (Join-Path $taskFixture 'start.cmd')
function Invoke-EntryCheck {
    $taskProcess = Start-Process -FilePath $env:ComSpec -ArgumentList @('/d','/c',('""{0}" /check"' -f (Join-Path $taskFixture 'start.cmd'))) -Wait -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskFixture 'stdout.txt') -RedirectStandardError (Join-Path $taskFixture 'stderr.txt')
    $taskProcess.ExitCode
}
if ((Invoke-EntryCheck) -eq 0) { throw 'Missing runtime incorrectly passed /check' }
foreach ($taskRelative in @('target\x86_64-pc-windows-msvc\debug\shixu-desktop.exe',
    'target\x86_64-pc-windows-msvc\debug\vault-win-x64\runtime\node.exe',
    'target\x86_64-pc-windows-msvc\debug\vault-win-x64\helper\helper.mjs',
    'tools\NapCat.Framework\napimain.exe', 'tools\NapCat.Framework\napiloader.dll',
    'tools\NapCat.Framework\nativeLoader.cjs', 'tools\NapCat.Framework\napcat.mjs',
    'tools\NapCat.Shell\config\webui.json')) {
    $taskFile = Join-Path $taskFixture $taskRelative
    New-Item -ItemType Directory -Path (Split-Path $taskFile) -Force | Out-Null
    [IO.File]::WriteAllText($taskFile, '')
}
if ((Invoke-EntryCheck) -ne 0) { throw 'Framework runtime failed /check with a path containing spaces; default must preserve the QQ GUI' }
Write-Output 'PASS: actual CMD /check preserves failure exit code and supports spaces; no application launched'

$taskRuntimeDir = Join-Path $taskFixture 'tools\NapCat.Framework'
$taskNativeModule = Join-Path $taskRuntimeDir 'napiloader.dll'
$taskOtherModule = Join-Path $taskFixture 'other\napiloader.dll'
$taskFrameworkQQ = [pscustomobject]@{ Name='QQ'; Modules=@([pscustomobject]@{ FileName=$taskNativeModule }) }
$taskOtherQQ = [pscustomobject]@{ Name='QQ'; Modules=@([pscustomobject]@{ FileName=$taskOtherModule }) }
$taskShell = [pscustomobject]@{ Name='NapCatWinBootMain'; Path=(Join-Path $taskFixture 'tools\NapCat.Shell\NapCatWinBootMain.exe') }
if (-not (Test-ShixuNapCatRunning -Mode Framework -RuntimeDir $taskRuntimeDir -Processes @($taskFrameworkQQ))) { throw 'Framework QQ must be reused even after napimain exits' }
foreach ($taskUnrelated in @($taskOtherQQ,$taskShell)) {
    if (Test-ShixuNapCatRunning -Mode Framework -RuntimeDir $taskRuntimeDir -Processes @($taskUnrelated)) { throw 'Unrelated loader or old Shell must not count as project Framework' }
}
if (-not (Test-ShixuNapCatRunning -Mode Shell -RuntimeDir (Join-Path $taskFixture 'tools\NapCat.Shell') -Processes @($taskShell))) { throw 'Explicit Shell fallback should reuse its project process' }
Write-Output 'PASS: reuse GUI QQ via loaded project module; distinguish another loader and old Shell'
