# Fixed Windows minimum workflow; synthetic probe only, never enables production.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Set-Location (Split-Path -Parent $PSScriptRoot)
$evidence = Join-Path '.superpowers/sdd/shixu-v0.1' ('task-windows-vault-lifecycle-native-' + [Guid]::NewGuid().ToString('N'))
[void](New-Item -ItemType Directory -Path $evidence)
function Write-NewText([string]$Name, [string]$Text) {
    $stream = [IO.File]::Open((Join-Path $evidence $Name), [IO.FileMode]::CreateNew, [IO.FileAccess]::Write)
    try { $writer = New-Object IO.StreamWriter($stream, [Text.UTF8Encoding]::new($false)); $writer.Write($Text); $writer.Flush() } finally { $stream.Dispose() }
}
function Invoke-Recorded([string]$Name, [string]$Program, [string[]]$Arguments) {
    $start = [DateTime]::UtcNow
    $ErrorActionPreference = 'Continue'
    $output = @(& $Program @Arguments 2>&1 | ForEach-Object { $_.ToString() })
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    Write-NewText ($Name + '.log') ($output -join "`n")
    Write-NewText ($Name + '.json') ((@{ program=$Program; argv=$Arguments; exit=$code; started_utc=$start.ToString('o'); ended_utc=[DateTime]::UtcNow.ToString('o') } | ConvertTo-Json -Depth 8) + "`n")
    if ($code -ne 0) { throw "BLOCKED/FAIL: $Name direct exit=$code; see $evidence; no dependency install/retry attempted" }
    return ($output -join "`n")
}
trap { Write-NewText 'failure.txt' ($_.Exception.Message + "`n"); throw }
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT -or -not [Environment]::Is64BitProcess -or -not [Environment]::Is64BitOperatingSystem) { throw 'BLOCKED: actual Windows x64 required' }
$principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'BLOCKED: standard-user x64 VS Developer PowerShell required' }
if ($env:VSCMD_ARG_TGT_ARCH -ne 'x64') { throw 'BLOCKED: open installed x64 VS C++/SDK Developer PowerShell first' }
if ($env:LIBSQLITE3_SYS_USE_PKG_CONFIG -or $env:PKG_CONFIG_ALLOW_CROSS) { throw 'BLOCKED: Linux metadata overrides are not native evidence' }
foreach ($tool in @('git','cargo','rustc','rustup','cl.exe','link.exe','rc.exe','python','node','npm.cmd','pnpm.cmd')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "BLOCKED: missing installed $tool; prepare prerequisite separately" }
}
$targets = Invoke-Recorded 'targets' 'rustup' @('target','list','--installed')
if ($targets -notmatch '(?m)^x86_64-pc-windows-msvc\s*$') { throw 'BLOCKED: MSVC Rust target not installed' }
$compiler = Invoke-Recorded 'compiler' 'rustc' @('-vV')
if ($compiler -notmatch '(?m)^release: 1\.99\.0\s*$' -or $compiler -notmatch '(?m)^host: x86_64-pc-windows-msvc\s*$') { throw 'BLOCKED: Rust 1.99.0 MSVC host required' }
$node = Invoke-Recorded 'node-version' 'node' @('--version')
$pnpm = Invoke-Recorded 'pnpm-version' 'pnpm.cmd' @('--version')
if ($node.Trim() -ne 'v24.19.0' -or $pnpm.Trim() -ne '11.19.0') { throw 'BLOCKED: installed frontend Node24.19.0/pnpm11.19.0 required; no installer runs here' }
$head = Invoke-Recorded 'head' 'git' @('rev-parse','HEAD')
$dirty = Invoke-Recorded 'status' 'git' @('status','--porcelain')
if ($dirty.Trim()) { throw 'BLOCKED: source must be reviewed and committed before evidence' }
if (-not (Test-Path 'node_modules/.modules.yaml' -PathType Leaf)) { throw 'BLOCKED: frozen frontend dependencies unprepared; prepare separately from the public pnpm lock' }
# Require exact public runtime archive cache before explicit offline preparation.
$cache = Join-Path ([IO.Path]::GetTempPath()) 'shixu-vault-runtime-downloads'
$archive = Join-Path $cache 'node-v26.11.1-win-x64.zip'
if (-not (Test-Path -LiteralPath $archive -PathType Leaf)) { throw 'BLOCKED: cache pinned official Node26.11.1 Windows archive via documented explicit preparation first' }
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne '97f36a8a9684ff0d3e35758b4610fef5b628a5880e96f2ccc11240e5daf9934e') { throw 'BLOCKED: public runtime archive hash mismatch' }
$null = Invoke-Recorded 'cargo-cache' 'cargo' @('metadata','--locked','--offline','--format-version','1','--filter-platform','x86_64-pc-windows-msvc')
$null = Invoke-Recorded 'icon' 'python' @('scripts/generate-engineering-icon.py','--check')
$null = Invoke-Recorded 'icon-structure' 'python' @('scripts/test-engineering-icon.py')
$null = Invoke-Recorded 'prepare' 'python' @('scripts/prepare-vault-runtime.py','--platform','win-x64','--offline')
$null = Invoke-Recorded 'resources' 'python' @('scripts/prepare-vault-runtime.py','--platform','win-x64','--verify-only')
$null = Invoke-Recorded 'typecheck' 'pnpm.cmd' @('run','typecheck')
$null = Invoke-Recorded 'frontend' 'pnpm.cmd' @('run','build')
# Verify the actual trusted constructors on Windows before the expensive native probe.
$paths = Invoke-Recorded 'trusted-path-constructors' 'cargo' @('test','-p','shixu-native','--lib','--target','x86_64-pc-windows-msvc','--locked','--offline','vault::windows::tests::actual_trusted_constructors_pass_strict_windows_admission','--','--exact','--nocapture','--test-threads=1')
if ($paths -notmatch 'running 1 test' -or $paths -notmatch '1 passed; 0 failed; 0 ignored') { throw 'FAIL: trusted path constructor regression must actually run once' }
# Existing runner validates exact 1 native test and all 33 fixed case families;
# each of its direct Cargo exits and source/artifact identities is preserved.
& (Join-Path $PSScriptRoot 'test-windows-vault-boundary.ps1')
if (-not $?) { throw 'BLOCKED/FAIL: native synthetic boundary runner failed' }
$null = Invoke-Recorded 'desktop-msvc' 'cargo' @('build','-p','shixu-desktop','--target','x86_64-pc-windows-msvc','--locked','--offline','--features','custom-protocol')
$controllerPrefix = @('test','-p','shixu-desktop','--lib','--target','x86_64-pc-windows-msvc','--locked','--offline','--features','custom-protocol')
$controllerCases = @(
    @('blocked-reveal','vault::tests::lifecycle_barrier_returns_during_blocked_reveal_and_requires_fresh_unlock'),
    @('serialized-reply','vault::tests::lifecycle_after_serialization_prevents_actual_submission'),
    @('blocked-startup','vault::tests::lifecycle_invalidates_blocked_startup_and_disabled_consumer_stays_closed')
)
foreach ($case in $controllerCases) {
    $output = Invoke-Recorded $case[0] 'cargo' ($controllerPrefix + @($case[1],'--','--exact','--nocapture','--test-threads=1'))
    if ($output -notmatch 'running 1 test' -or $output -notmatch '1 passed; 0 failed; 0 ignored') { throw 'FAIL: each synthetic controller probe must actually run once' }
}
# These controlled-engine tests exercise window-close revocation and reply guards.
# Session lock and suspend/resume handling are outside the user-approved scope.
$destination = Join-Path (Get-Location).Path 'target/x86_64-pc-windows-msvc/debug/vault-win-x64'
$manifest = Get-Content 'vault-helper/resources-win-x64.json' -Raw | ConvertFrom-Json
$files = @(Get-ChildItem -LiteralPath $destination -Recurse -Force)
if ($files | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }) { throw 'FAIL: built resource reparse point' }
$names = @($files | Where-Object { -not $_.PSIsContainer } | ForEach-Object { $_.FullName.Substring($destination.Length + 1).Replace([char]92,[char]47) } | Sort-Object)
$expected = @($manifest.files.PSObject.Properties.Name | Sort-Object)
if ($names.Count -ne 198 -or (Compare-Object $names $expected)) { throw 'FAIL: copied runtime keys differ from reviewed 198-file tree' }
$resourceHashes = @{}
foreach ($entry in $manifest.files.PSObject.Properties) {
    $digest = (Get-FileHash -LiteralPath (Join-Path $destination $entry.Name) -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($digest -ne $entry.Value) { throw 'FAIL: built runtime hash mismatch' }
    $resourceHashes[$entry.Name] = $digest
}
$sourceHashes = @{}
# Every tracked source file; paths and hashes only, no secret file contents.
$sourceNames = @(& git ls-files)
if ($LASTEXITCODE -ne 0) { throw 'FAIL: source identity unavailable' }
foreach ($name in $sourceNames) { $sourceHashes[$name] = (Get-FileHash -LiteralPath $name -Algorithm SHA256).Hash.ToLowerInvariant() }
$exe = 'target/x86_64-pc-windows-msvc/debug/shixu-desktop.exe'
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'FAIL: desktop artifact missing after successful build' }
Write-NewText 'result.json' ((@{ head=$head.Trim(); source_sha256=$sourceHashes; artifact_sha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant(); resource_sha256=$resourceHashes; production='Unsupported'; Q1='OPEN'; release_gates=@(1..12 | ForEach-Object { 'BLOCKED' }); trusted_path_constructor_count=1; synthetic_controller_count=3; session_power_scope='REMOVED_BY_USER'; manual_window_tray='NOT EXECUTED BY THIS SCRIPT' } | ConvertTo-Json -Depth 8) + "`n")
Write-Output "Minimum MSVC build/native synthetic evidence=$evidence. Production Unsupported; Q1 OPEN; all12 gates BLOCKED. Optional window/tray observation: scripts/observe-windows-vault-lifecycle.ps1"
