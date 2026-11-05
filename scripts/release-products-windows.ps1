[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('open-3d-viewer', 'open-bim-validator')][string]$Product,
    [Parameter(Mandatory)][string]$Stage,
    [Parameter(Mandatory)][string]$Output,
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$Stem,
    [Parameter(Mandatory)][string]$Evidence
)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent
$sign = Join-Path $PSScriptRoot 'windows-sign.ps1'
& $sign -Check
$nsis = (Get-Command makensis -ErrorAction SilentlyContinue).Source
if (!$nsis) { $nsis = "${env:ProgramFiles(x86)}\NSIS\makensis.exe" }
if (!(Test-Path -LiteralPath $nsis)) { throw 'NSIS 3.08+ is required.' }
$binaries = @("$Product.exe")
if ($Product -eq 'open-3d-viewer') { $binaries += 'open-3d-viewer-mcp.exe' }
$files = @($binaries | ForEach-Object { Join-Path $Stage $_ })
$productName = if ($Product -eq 'open-3d-viewer') { 'Open 3D Viewer Studio' } else { 'Open BIM Validator' }
foreach ($file in $files) {
    if ([Diagnostics.FileVersionInfo]::GetVersionInfo($file).ProductName -ne $productName) {
        throw "Executable product identity mismatch: $file"
    }
}
& $sign -Files $files
$recipe = if ($Product -eq 'open-3d-viewer') { 'installer.nsi' } else { 'validator.nsi' }
$setup = Join-Path $Output "$Stem-setup.exe"
& $nsis "/DNUMBER=$Version" "/DSOURCE=$Stage" "/DOUTPUT=$setup" "/DSIGN=$sign" (Join-Path $repoRoot "packaging/windows/$recipe")
if ($LASTEXITCODE -ne 0) { throw 'Signed installer/uninstaller build failed.' }
& $sign -Files @($setup)

# Verify the installed payload and the embedded uninstaller, not just the outer package.
$installRoot = Join-Path $env:RUNNER_TEMP ("verify-" + $Product + '-' + [guid]::NewGuid().ToString('N'))
$process = Start-Process -FilePath $setup -ArgumentList '/S', "/D=$installRoot" -WindowStyle Hidden -Wait -PassThru
if ($process.ExitCode -ne 0) { throw "Installer failed: $($process.ExitCode)" }
$verified = @()
foreach ($file in @($files) + @($setup) + @($binaries | ForEach-Object { Join-Path $installRoot $_ }) + @(Join-Path $installRoot 'uninstall.exe')) {
    $signature = Get-AuthenticodeSignature -LiteralPath $file
    if ($signature.Status -ne 'Valid' -or !$signature.TimeStamperCertificate) {
        throw "Valid timestamped Authenticode signature required: $file"
    }
    $verified += @{ file = Split-Path $file -Leaf; subject = $signature.SignerCertificate.Subject; thumbprint = $signature.SignerCertificate.Thumbprint }
}
@{ method = 'authenticode-timestamped'; verified = $true; files = $verified } |
    ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $Evidence -Encoding utf8
