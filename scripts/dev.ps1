param(
    [string]$Model = 'test-models/TestModel.skp',
    [switch]$Stop,
    [switch]$Once,
    [int]$Jobs = 4
)

# A local native dev viewer. Source changes rebuild incrementally and restart
# only the process started here. A failed build leaves the last working app open.
$projectRoot = Split-Path -Parent $PSScriptRoot
$outputRoot = Join-Path $projectRoot 'target/dev'
New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
$stopFile = Join-Path $outputRoot 'stop'
if ($Stop) { Set-Content -LiteralPath $stopFile -Value 'stop'; return }
if (Test-Path -LiteralPath $stopFile) { Remove-Item -LiteralPath $stopFile }
$watchLock = [IO.File]::Open((Join-Path $outputRoot 'watch.lock'), 'OpenOrCreate', 'ReadWrite', 'Read')
$viewerProcess = $null
$statusFile = Join-Path $outputRoot 'status.json'
$buildLog = Join-Path $outputRoot 'build.log'

function Write-DevStatus([string]$Phase, [string]$Detail) {
    @{ phase = $Phase; detail = $Detail; time = [DateTime]::UtcNow.ToString('o'); watcherPid = $PID;
       viewerPid = $(if ($viewerProcess) { $viewerProcess.Id } else { $null }) } |
        ConvertTo-Json | Set-Content -LiteralPath $statusFile -Encoding utf8
    Write-Host "[$(Get-Date -Format HH:mm:ss)] $Detail"
}

function Get-SourceStamp {
    $files = Get-ChildItem -LiteralPath (Join-Path $projectRoot 'src'), (Join-Path $projectRoot 'assets'),
        (Join-Path $projectRoot 'vendor/openaec-ui/src') -Recurse -File
    $files += Get-Item -LiteralPath (Join-Path $projectRoot 'Cargo.toml'), (Join-Path $projectRoot 'Cargo.lock'),
        (Join-Path $projectRoot 'build.rs'), (Join-Path $projectRoot '.cargo/config.toml')
    ($files | Sort-Object FullName | ForEach-Object { "$($_.FullName)|$($_.Length)|$($_.LastWriteTimeUtc.Ticks)" }) -join "`n"
}

try {
    Set-Location -LiteralPath $projectRoot
    $modelPath = (Resolve-Path -LiteralPath $Model -ErrorAction Stop).Path
    $portableRust = Join-Path $projectRoot 'temp/cargo/bin/cargo.exe'
    $portableCompiler = Join-Path $projectRoot 'temp/dev-toolchain/mingw64/bin'
    $usePortable = (Test-Path -LiteralPath $portableRust) -and (Test-Path -LiteralPath (Join-Path $portableCompiler 'gcc.exe'))
    $target = @()
    $targetFolder = ''
    if ($usePortable) {
        $env:CARGO_HOME = Join-Path $projectRoot 'temp/cargo'
        $env:RUSTUP_HOME = Join-Path $projectRoot 'temp/rustup'
        $env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-gnu'
        $env:PATH = "$portableCompiler;$env:PATH"
        $cargo = $portableRust
        $target = @('--target', 'x86_64-pc-windows-gnu')
        $targetFolder = 'x86_64-pc-windows-gnu'
    } else {
        $cargo = (Get-Command cargo -ErrorAction Stop).Source
    }
    $env:CARGO_TARGET_DIR = $outputRoot
    $env:CARGO_BUILD_JOBS = [string][Math]::Max(1, $Jobs)
    $env:CARGO_PROFILE_DEV_DEBUG = '1'
    $env:CARGO_PROFILE_DEV_INCREMENTAL = 'true'
    $viewerExe = Join-Path (Join-Path $outputRoot $targetFolder) 'debug/open-3d-viewer.exe'
    $previousStamp = ''
    while (!(Test-Path -LiteralPath $stopFile)) {
        $stamp = Get-SourceStamp
        if ($stamp -ne $previousStamp) {
            # Debounce editor saves; changes during compilation are picked up next.
            Start-Sleep -Milliseconds 500
            $previousStamp = Get-SourceStamp
            Write-DevStatus 'building' 'Building the latest local UI...'
            & $cargo build --locked --bin open-3d-viewer @target *>> $buildLog
            $buildExitCode = $LASTEXITCODE
            if ($buildExitCode -eq 0) {
                if ($viewerProcess -and !$viewerProcess.HasExited) { Stop-Process -Id $viewerProcess.Id }
                $arguments = @(('"' + $modelPath + '"'), '--no-remote')
                $viewerProcess = Start-Process -FilePath $viewerExe -ArgumentList $arguments -WorkingDirectory $projectRoot -PassThru
                Write-DevStatus 'running' 'Dev viewer updated; watching source files.'
            } else {
                Write-DevStatus 'error' "Build failed; the previous viewer stays open. See $buildLog"
            }
            if ($Once) { if ($buildExitCode -ne 0) { exit $buildExitCode }; break }
        }
        Start-Sleep -Seconds 1
    }
} finally {
    if (!$Once -and $viewerProcess -and !$viewerProcess.HasExited) { Stop-Process -Id $viewerProcess.Id }
    $watchLock.Dispose()
}
