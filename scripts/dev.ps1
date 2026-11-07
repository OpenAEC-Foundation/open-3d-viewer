param(
    # The file to open; without it the viewer opens the files that were open before its restart
    # (target/dev/session.txt), or the test model the first time.
    [string]$Model = '',
    [switch]$Stop,
    [switch]$Once,
    # Parallel compile jobs; 0 leaves it to cargo.
    [int]$Jobs = 0
)

# A local native dev viewer. Source changes rebuild incrementally (the normal target/ cache, shared
# with `cargo build` and `cargo test` in this checkout) and restart only the viewer started here,
# which then opens the same files in the same tabs. The viewer runs from a copy of the build, so the
# next build can link while it is open. A failed build leaves the last working viewer open.
$projectRoot = Split-Path -Parent $PSScriptRoot
$outputRoot = Join-Path $projectRoot 'target/dev'
New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
$stopFile = Join-Path $outputRoot 'stop'
if ($Stop) { Set-Content -LiteralPath $stopFile -Value 'stop'; return }
# one watcher per checkout; a second one leaves the first (and its stop request) alone
try {
    $watchLock = [IO.File]::Open((Join-Path $outputRoot 'watch.lock'), 'OpenOrCreate', 'ReadWrite', 'None')
} catch {
    Write-Host 'Another dev watcher runs in this checkout; stop it with scripts/dev.ps1 -Stop (it stops after its build).'
    exit 1
}
if (Test-Path -LiteralPath $stopFile) { Remove-Item -LiteralPath $stopFile }
$viewerProcess = $null
$statusFile = Join-Path $outputRoot 'status.json'
$buildLog = Join-Path $outputRoot 'build.log'
$sessionFile = Join-Path $outputRoot 'session.txt'
$runFolder = Join-Path $outputRoot 'run'

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

function Stop-Viewer {
    if ($script:viewerProcess -and !$script:viewerProcess.HasExited) {
        Stop-Process -Id $script:viewerProcess.Id -ErrorAction SilentlyContinue
        $script:viewerProcess.WaitForExit(5000) | Out-Null
    }
}

try {
    Set-Location -LiteralPath $projectRoot
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
        # the MSVC toolchain, as the README asks, also where GNU is the default: GNU links a debug
        # build of the viewer many times slower
        if (!$env:RUSTUP_TOOLCHAIN -and ((& rustup toolchain list 2>$null) -match '^stable-x86_64-pc-windows-msvc')) {
            $env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-msvc'
        }
    }
    if ($Jobs -gt 0) { $env:CARGO_BUILD_JOBS = [string]$Jobs }
    $builtExe = Join-Path (Join-Path (Join-Path $projectRoot 'target') $targetFolder) 'debug/open-3d-viewer.exe'
    $viewerExe = Join-Path $runFolder 'open-3d-viewer.exe'
    # the viewer keeps its open files here and shows what it is a build of in its title
    $env:O3V_DEV_SESSION = $sessionFile
    $branch = (& git -C $projectRoot branch --show-current 2>$null)
    $env:O3V_DEV_LABEL = $(if ($branch) { "dev $branch" } else { 'dev' })
    if ($Model) {
        $modelPath = (Resolve-Path -LiteralPath $Model -ErrorAction Stop).Path
    } elseif (!(Test-Path -LiteralPath $sessionFile)) {
        $modelPath = (Resolve-Path -LiteralPath 'test-models/TestModel.skp' -ErrorAction Stop).Path
    } else {
        $modelPath = ''
    }
    $previousStamp = ''
    while (!(Test-Path -LiteralPath $stopFile)) {
        $stamp = Get-SourceStamp
        if ($stamp -ne $previousStamp) {
            # Debounce editor saves; changes during compilation are picked up next.
            Start-Sleep -Milliseconds 300
            $previousStamp = Get-SourceStamp
            Write-DevStatus 'building' 'Building the latest local UI...'
            $started = Get-Date
            & $cargo build --locked --bin open-3d-viewer @target *> $buildLog
            $buildExitCode = $LASTEXITCODE
            if ($buildExitCode -eq 0) {
                Stop-Viewer
                New-Item -ItemType Directory -Path $runFolder -Force | Out-Null
                Copy-Item -LiteralPath $builtExe -Destination $viewerExe -Force
                # the file of -Model the first time; after that the files the viewer kept open
                $arguments = @('--no-remote')
                if ($modelPath) { $arguments = @(('"' + $modelPath + '"')) + $arguments; $modelPath = '' }
                $viewerProcess = Start-Process -FilePath $viewerExe -ArgumentList $arguments -WorkingDirectory $projectRoot -PassThru
                $seconds = [Math]::Round(((Get-Date) - $started).TotalSeconds)
                Write-DevStatus 'running' "Dev viewer updated after a $seconds s build; watching source files."
            } else {
                $recentOutput = (Get-Content -LiteralPath $buildLog -Tail 60) -join "`n"
                if ($recentOutput -match 'os error 4551|Application Control policy') {
                    Write-DevStatus 'blocked' 'Windows Application Control blocked a build executable; an approved local build environment is required.'
                    exit $buildExitCode
                }
                Write-DevStatus 'error' "Build failed; the previous viewer stays open. See $buildLog"
            }
            if ($Once) { if ($buildExitCode -ne 0) { exit $buildExitCode }; break }
        }
        Start-Sleep -Milliseconds 500
    }
} finally {
    if (!$Once -and (Test-Path -LiteralPath $stopFile)) {
        Stop-Viewer
        Write-DevStatus 'stopped' 'Dev watcher stopped.'
    }
    $watchLock.Dispose()
}
