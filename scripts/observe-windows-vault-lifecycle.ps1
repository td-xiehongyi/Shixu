# Fixed unavailable-vault observation; no credentials, forced lock or suspend.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Set-Location (Split-Path -Parent $PSScriptRoot)
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT -or -not [Environment]::Is64BitProcess) { throw 'BLOCKED: interactive Windows x64 required' }
$principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'BLOCKED: use standard-user interactive PowerShell' }
$webview = @('HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\*','HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\*','HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\*') | ForEach-Object { Get-ItemProperty $_ -ErrorAction SilentlyContinue } | Where-Object { $_.PSObject.Properties['name'] -and $_.PSObject.Properties['pv'] -and $_.name -like '*WebView2*' -and $_.pv -and $_.pv -ne '0.0.0.0' }
if (-not $webview) { throw 'BLOCKED: installed WebView2 runtime not found; prepare prerequisite separately' }
$exe = Join-Path (Get-Location).Path 'target/x86_64-pc-windows-msvc/debug/shixu-desktop.exe'
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'BLOCKED: run successful reviewed minimum workflow first' }
$evidence = Join-Path '.superpowers/sdd/shixu-v0.1' ('task-windows-vault-lifecycle-observe-' + [Guid]::NewGuid().ToString('N'))
[void](New-Item -ItemType Directory -Path $evidence)
$process = Start-Process -FilePath $exe -PassThru -RedirectStandardOutput (Join-Path $evidence 'desktop.stdout.log') -RedirectStandardError (Join-Path $evidence 'desktop.stderr.log')
Write-Output 'Open the unavailable vault window. Manually close main to tray, lock/unlock Windows, then suspend/resume once using Windows UI. Do not enter credentials. Exit via tray after observing vault stays locked.'
$process.WaitForExit()
$result = @{ artifact_sha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant(); exit=$process.ExitCode; production='Unsupported'; release='BLOCKED'; observation='User manual actions; logs show dispatcher receipt, not production engine or OS protection acceptance' }
$path = Join-Path $evidence 'result.json'
$stream = [IO.File]::Open($path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write)
try { $writer = New-Object IO.StreamWriter($stream, [Text.UTF8Encoding]::new($false)); $writer.Write(($result | ConvertTo-Json) + "`n"); $writer.Flush() } finally { $stream.Dispose() }
Write-Output "Manual observation evidence=$evidence; review logs and actions separately; all release gates remain BLOCKED."
