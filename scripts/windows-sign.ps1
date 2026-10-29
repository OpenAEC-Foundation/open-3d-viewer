# OpenAEC Azure Artifact Signing convention, adapted from open-planner-studio:
# https://github.com/OpenAEC-Foundation/open-planner-studio/blob/main/scripts/windows-sign.ps1
# Uses the existing organization identity; never falls back to interactive login.
[CmdletBinding()]
param([string[]]$Files, [switch]$Check)

function Require-SigningEnvironment([string]$Name) {
    if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($Name))) {
        throw "Required organization signing setting is unavailable to this repository: $Name"
    }
}
foreach ($name in @('AZURE_TENANT_ID', 'AZURE_CLIENT_ID', 'AZURE_CLIENT_SECRET', 'AZURE_ENDPOINT',
    'AZURE_TRUSTED_SIGNING_ACCOUNT_NAME', 'AZURE_CERTIFICATE_PROFILE_NAME')) {
    Require-SigningEnvironment $name
}
if ($Check) { Write-Host 'Organization signing settings are available.'; return }
if (!$Files) { throw 'Provide the Windows executables to sign.' }
Require-SigningEnvironment 'RUNNER_TEMP'
$clientVersion = '1.0.146'
$clientSha512 = '/G0hG/VDIS6psW23b5t1V/ORIgn0PVAOtGj9w+hkPmAuI4pempfwlW50E2GtHscnz9+5Qz3hbC4mcb7ej2Yn1A=='
$signingRoot = Join-Path $env:RUNNER_TEMP 'viewer-artifact-signing'
New-Item -ItemType Directory -Path $signingRoot -Force | Out-Null
$package = Join-Path $signingRoot 'client.zip'
Invoke-WebRequest -Uri "https://api.nuget.org/v3-flatcontainer/microsoft.artifactsigning.client/$clientVersion/microsoft.artifactsigning.client.$clientVersion.nupkg" -OutFile $package -ErrorAction Stop
$actualHash = [Convert]::ToBase64String([Security.Cryptography.SHA512]::HashData([IO.File]::ReadAllBytes($package)))
if ($actualHash -ne $clientSha512) { throw 'Artifact Signing client checksum mismatch.' }
Expand-Archive -LiteralPath $package -DestinationPath (Join-Path $signingRoot 'client') -Force -ErrorAction Stop
$dlib = Join-Path $signingRoot 'client/bin/x64/Azure.CodeSigning.Dlib.dll'
if (!(Test-Path -LiteralPath $dlib)) { throw 'Microsoft signing library is missing.' }
$signtool = Get-ChildItem 'C:/Program Files (x86)/Windows Kits/10/bin/*/x64/signtool.exe' |
    Where-Object { $_.Directory.Parent.Name -match '^\d+\.\d+\.\d+\.\d+$' } |
    Sort-Object { [version]$_.Directory.Parent.Name } -Descending | Select-Object -First 1
if (!$signtool) { throw 'Windows SDK signtool.exe is missing.' }
$metadata = Join-Path $signingRoot 'metadata.json'
@{
    Endpoint = $env:AZURE_ENDPOINT
    CodeSigningAccountName = $env:AZURE_TRUSTED_SIGNING_ACCOUNT_NAME
    CertificateProfileName = $env:AZURE_CERTIFICATE_PROFILE_NAME
    ExcludeCredentials = @('ManagedIdentityCredential', 'WorkloadIdentityCredential', 'SharedTokenCacheCredential',
        'VisualStudioCredential', 'VisualStudioCodeCredential', 'AzureCliCredential', 'AzurePowerShellCredential',
        'AzureDeveloperCliCredential', 'InteractiveBrowserCredential')
} | ConvertTo-Json | Set-Content -LiteralPath $metadata -Encoding utf8
foreach ($file in $Files) {
    $resolvedFile = (Resolve-Path -LiteralPath $file -ErrorAction Stop).Path
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        & $signtool.FullName sign /fd SHA256 /tr http://timestamp.acs.microsoft.com /td SHA256 /dlib $dlib /dmdf $metadata $resolvedFile
        if ($LASTEXITCODE -eq 0) { break }
        if ($attempt -eq 3) { throw "Signing failed: $file" }
        Start-Sleep -Seconds (5 * $attempt)
    }
    & $signtool.FullName verify /pa /all $resolvedFile
    if ($LASTEXITCODE -ne 0) { throw "Authenticode verification failed: $file" }
    $signature = Get-AuthenticodeSignature -LiteralPath $resolvedFile
    if ($signature.Status -ne 'Valid' -or !$signature.TimeStamperCertificate) {
        throw "A valid timestamped Authenticode signature is required: $file"
    }
    Write-Host "Verified signature: $file ($($signature.SignerCertificate.Subject))"
}
