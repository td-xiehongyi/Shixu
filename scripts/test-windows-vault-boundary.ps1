# Actual Windows x64, standard-user synthetic-only probes. Never enables production.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Set-Location (Split-Path -Parent $PSScriptRoot)
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT -or -not [Environment]::Is64BitOperatingSystem -or -not [Environment]::Is64BitProcess) { throw 'BLOCKED: actual Windows x64 PowerShell required' }
# Evidence is unique and every file is exclusive-created. No environment dump.
$evidence = Join-Path '.superpowers/sdd/shixu-v0.1' ('task-windows-vault-boundary-native-' + [Guid]::NewGuid().ToString('N'))
[void](New-Item -ItemType Directory -Path $evidence)
function Write-NewText([string]$Name, [string]$Text) {
    $path = Join-Path $evidence $Name
    $stream = [IO.File]::Open($path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write)
    try { $writer = New-Object IO.StreamWriter($stream, [Text.UTF8Encoding]::new($false)); $writer.Write($Text); $writer.Flush() } finally { $stream.Dispose() }
}
function Invoke-Recorded([string]$Name, [string]$Program, [string[]]$Arguments) {
    $started = [DateTime]::UtcNow
    $ErrorActionPreference = 'Continue'
    # Compilation/test output only. Helpers suppress stderr and never print frames.
    $output = @(& $Program @Arguments 2>&1 | ForEach-Object { $_.ToString() })
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    Write-NewText ($Name + '.log') ($output -join "`n")
    Write-NewText ($Name + '.json') ((@{ program=$Program; argv=$Arguments; exit=$code; started_utc=$started.ToString('o'); ended_utc=[DateTime]::UtcNow.ToString('o') } | ConvertTo-Json -Depth 8) + "`n")
    if ($code -ne 0) { throw "FAIL/BLOCKED: $Name direct exit=$code; safe raw log retained in $evidence" }
    return ($output -join "`n")
}
trap {
    Write-NewText 'runner-failure.txt' ($_.Exception.Message + "`n")
    throw
}
$principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'BLOCKED: use a standard-user Developer Shell, not elevated PowerShell' }
if ($env:LIBSQLITE3_SYS_USE_PKG_CONFIG -or $env:PKG_CONFIG_ALLOW_CROSS) { throw 'BLOCKED: remove Linux SQLite cfg override; this requires real MSVC linking' }
foreach ($tool in @('git','cargo','rustc','rustup','cl.exe','link.exe','rc.exe')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "BLOCKED: missing $tool; prepare Rust 1.99 MSVC and x64 VS C++/SDK Developer Shell first" }
}
$targets = & rustup target list --installed
if ($LASTEXITCODE -ne 0 -or -not ($targets | Where-Object { $_.Trim() -eq 'x86_64-pc-windows-msvc' })) { throw 'BLOCKED: x86_64-pc-windows-msvc target not installed; prepare it explicitly first' }
$compiler = & rustc -vV
if ($LASTEXITCODE -ne 0 -or -not ($compiler -match '^host: x86_64-pc-windows-msvc$') -or -not ($compiler -match '^release: 1\.99\.0$')) { throw 'BLOCKED: repository requires Rust 1.99.0 with Windows MSVC host' }
$head = Invoke-Recorded 'head' 'git' @('rev-parse','HEAD')
$dirty = Invoke-Recorded 'status' 'git' @('status','--porcelain')
if ($dirty.Trim()) { throw 'BLOCKED: tracked worktree differs from committed source; commit reviewed changes before collecting evidence' }
$manifest = Get-Content -LiteralPath 'vault-helper/resources-win-x64.json' -Raw | ConvertFrom-Json
if ($manifest.version -ne 1 -or $manifest.node -ne '26.11.1' -or $manifest.kdbxweb -ne '2.1.1' -or $manifest.hash_wasm -ne '4.12.0' -or $manifest.archive_sha256 -ne '97f36a8a9684ff0d3e35758b4610fef5b628a5880e96f2ccc11240e5daf9934e') { throw 'BLOCKED: reviewed Windows runtime pins mismatch' }
$root = Join-Path (Get-Location).Path 'resources/vault-win-x64'
if (-not (Test-Path -LiteralPath $root -PathType Container)) { throw 'BLOCKED: pinned Windows resources missing; prepare them explicitly with Python/npm before this offline runner' }
$actual = @(Get-ChildItem -LiteralPath $root -Recurse -Force)
if ($actual | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }) { throw 'BLOCKED: resource reparse point' }
$actualNames = @($actual | Where-Object { -not $_.PSIsContainer } | ForEach-Object { $_.FullName.Substring($root.Length + 1).Replace([char]92,[char]47) } | Sort-Object)
$expectedNames = @($manifest.files.PSObject.Properties.Name | Sort-Object)
if ($actualNames.Count -ne 198 -or (Compare-Object $actualNames $expectedNames)) { throw 'BLOCKED: resource set differs from exact reviewed 198 files' }
foreach ($entry in $manifest.files.PSObject.Properties) {
    $path = Join-Path $root $entry.Name
    if ((Get-Item -LiteralPath $path).Length -gt 268435456 -or (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.Value) { throw 'BLOCKED: prepared resource size/hash mismatch' }
}
if ((Get-FileHash -LiteralPath (Join-Path $root 'runtime/node.exe') -Algorithm SHA256).Hash.ToLowerInvariant() -ne 'a619e2e09eb0d50ef4c733d678b4d22689d2afef138cd67fd3fa107b535f8819') { throw 'BLOCKED: pinned Node binary mismatch' }
$sourceNames = @('Cargo.lock','rust-toolchain.toml','crates/shixu-native/Cargo.toml','crates/shixu-native/src/vault/windows.rs','crates/shixu-native/src/vault/windows_policy.rs','crates/shixu-native/src/vault/engine.rs','crates/shixu-native/src/vault/pipe.rs','vault-helper/resources-win-x64.json','scripts/test-windows-vault-boundary.ps1')
$sourceHashes = @{}; foreach ($name in $sourceNames) { $sourceHashes[$name] = (Get-FileHash -LiteralPath $name -Algorithm SHA256).Hash.ToLowerInvariant() }
Write-NewText 'identity.json' ((@{ head=$head.Trim(); scoped_source_sha256=$sourceHashes; runtime_sha256=$manifest.files.'runtime/node.exe'; windows_version=[Environment]::OSVersion.Version.ToString(); compiler=$compiler } | ConvertTo-Json -Depth 8) + "`n")
$env:RUST_BACKTRACE = '0'
$prefix = @('test','-p','shixu-native','--lib','--target','x86_64-pc-windows-msvc','--locked','--offline')
$gate = Invoke-Recorded 'production-gate' 'cargo' ($prefix + @('vault::windows::tests::production_constructor_remains_blocked','--','--exact','--nocapture','--test-threads=1'))
if ($gate -notmatch 'running 1 test' -or $gate -notmatch '1 passed; 0 failed; 0 ignored') { throw 'FAIL: exact production gate must run once; zero/skipped matching tests are not evidence' }
$native = Invoke-Recorded 'native-boundary' 'cargo' ($prefix + @('vault::windows::tests::synthetic_native_boundary','--','--exact','--ignored','--nocapture','--test-threads=1'))
if ($native -notmatch 'running 1 test' -or $native -notmatch '1 passed; 0 failed; 0 ignored') { throw 'FAIL: exact native parent must run once; zero/skipped tests are not evidence' }
$record = @($native -split "`n" | Where-Object { $_ -match 'SHIXU_NATIVE_BOUNDARY ' })
if ($record.Count -ne 1) { throw 'FAIL: missing unique native case record' }
$cases = $record[0].Substring($record[0].IndexOf('SHIXU_NATIVE_BOUNDARY ') + 'SHIXU_NATIVE_BOUNDARY '.Length) | ConvertFrom-Json
if ($cases.production -ne 'Unsupported' -or $cases.passed_cases.Count -ne 33 -or @($cases.passed_cases | Sort-Object -Unique).Count -ne $cases.passed_cases.Count) { throw 'FAIL: native case accounting or production gate record mismatch' }
# Preserve actual executable identity from Cargo's test binary output.
$artifactLine = @($native -split "`n" | Where-Object { $_ -match 'Running unittests .*\((.+\.exe)\)' })
if ($artifactLine.Count -ne 1) { throw 'FAIL: native artifact identity unavailable' }
$artifactMatch = [regex]::Match($artifactLine[0], '\((.+\.exe)\)')
if (-not $artifactMatch.Success) { throw 'FAIL: native artifact identity unavailable' }
$artifact = $artifactMatch.Groups[1].Value
Write-NewText 'result.json' ((@{ head=$head.Trim(); artifact_sha256=(Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant(); passed_count=$cases.passed_cases.Count; blocked_count=$cases.blocked_cases.Count; cases=$cases; production='Unsupported'; release='BLOCKED' } | ConvertTo-Json -Depth 8) + "`n")
Write-Output "Synthetic Windows cases executed; evidence=$evidence; production remains Unsupported; release BLOCKED."
