# Operator-invoked validation only. Never builds, installs, logs in, sends, uploads or publishes.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [string]$Evidence,
    [Parameter(Mandatory = $true)][string]$Output
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) {
    Write-Error 'WINDOWS_BLOCKED: use PowerShell 7 on Windows; Linux validation cannot close native gates.' -ErrorAction Continue
    exit 2
}
try {
    $binary = (Resolve-Path -LiteralPath $Executable).Path
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw 'Missing executable' }
    $reportPath = [IO.Path]::GetFullPath($Output)
    $evidencePath = '-'
    if ($Evidence) { $evidencePath = (Resolve-Path -LiteralPath $Evidence).Path }
    if ($reportPath -eq $binary -or $reportPath -eq $evidencePath) { throw 'Output must be a separate report file' }
    # The exact Shixu executable embeds source/version/capability identity and
    # hashes its own bytes. An external JSON cannot supply the expected manifest.
    & $binary '--verify-release' $evidencePath $reportPath
    $code = $LASTEXITCODE
    if ($null -eq $code) { throw 'No direct executable exit status' }
    exit $code
} catch {
    # Avoid echoing file paths or arbitrary evidence/error text into logs.
    Write-Error 'FAIL: executable/evidence/report validation could not run. Check local paths and permissions.' -ErrorAction Continue
    exit 1
}
