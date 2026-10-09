# Fetch and verify the pinned Microsoft VC++ 2015-2022 x64 redistributable
# into src-tauri/resources/vc_redist.x64.exe (bundled by tauri.windows.conf.json).
# Fail closed: any mismatch deletes the download and exits 1.
$ErrorActionPreference = 'Stop'
# PS 5.1's progress bar makes Invoke-WebRequest dramatically slower.
$ProgressPreference = 'SilentlyContinue'

$Url    = "https://download.visualstudio.microsoft.com/download/pr/bd1c8d9d-ba95-4eee-bc6e-df1fcc876373/CC0FF0EB1DC3F5188AE6300FAEF32BF5BEEBA4BDD6E8E445A9184072096B713B/VC_redist.x64.exe"
$Sha256 = "cc0ff0eb1dc3f5188ae6300faef32bf5beeba4bdd6e8e445a9184072096b713b"
$Size   = 25635768
$Target = "$PSScriptRoot\..\src-tauri\resources\vc_redist.x64.exe"   # i.e. src-tauri/resources/vc_redist.x64.exe

# Returns $null when every check passes, otherwise a message naming the failed check.
function Test-Redist([string]$path) {
    $len = (Get-Item -LiteralPath $path).Length
    if ($len -ne $Size) { return "size check failed: $len bytes, expected $Size" }

    $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
    if ($hash -ine $Sha256) { return "SHA256 check failed: got $hash, expected $Sha256" }

    $sig = Get-AuthenticodeSignature -LiteralPath $path
    if ($sig.Status -ne 'Valid') { return "Authenticode status is '$($sig.Status)', expected 'Valid'" }
    if (-not $sig.SignerCertificate) { return "no signer certificate" }
    if ($sig.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') {
        return "signer is '$($sig.SignerCertificate.Subject)', expected O=Microsoft Corporation"
    }

    $chain = New-Object System.Security.Cryptography.X509Certificates.X509Chain
    # NoCheck: revocation lookups need the network and would make offline CI flaky;
    # integrity is already pinned by SHA256 + a valid signature.
    $chain.ChainPolicy.RevocationMode = [System.Security.Cryptography.X509Certificates.X509RevocationMode]::NoCheck
    [void]$chain.Build($sig.SignerCertificate)
    $n = $chain.ChainElements.Count
    if ($n -lt 1) { return "empty certificate chain" }
    $root = $chain.ChainElements[$n - 1].Certificate.Subject
    if ($root -notmatch 'CN=Microsoft Root Certificate Authority') {
        return "chain root is '$root', expected CN=Microsoft Root Certificate Authority"
    }
    $script:Signer = $sig.SignerCertificate.Subject
    $script:Root   = $root
    return $null
}

if (Test-Path -LiteralPath $Target) {
    $why = Test-Redist $Target
    if (-not $why) {
        Write-Host "fetch-vcredist: already present, verified ($Sha256)"
        exit 0
    }
    Write-Host "fetch-vcredist: existing file failed verification ($why); re-downloading"
}

$dir = Split-Path -Parent $Target
if (-not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Path $dir | Out-Null }

$tmp = [IO.Path]::GetTempFileName()
try {
    $ok = $false
    foreach ($attempt in 1..2) {   # retry once on a transient error
        try {
            Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $tmp
            $ok = $true
            break
        } catch {
            Write-Host "fetch-vcredist: download attempt $attempt failed: $($_.Exception.Message)"
        }
    }
    if (-not $ok) {
        Write-Error "fetch-vcredist: download failed after 2 attempts"
        Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
        exit 1
    }

    $why = Test-Redist $tmp
    if ($why) {
        # A Microsoft re-release under the pinned URL is impossible (the sha is in the
        # path), so a mismatch means tampering or corruption. Bump the pin deliberately.
        Write-Host "fetch-vcredist: VERIFICATION FAILED - $why"
        Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
        exit 1
    }
    Move-Item -LiteralPath $tmp -Destination $Target -Force
    Write-Host "fetch-vcredist: OK sha256=$Sha256 signer=$script:Signer root=$script:Root"
    exit 0
} catch {
    Write-Host "fetch-vcredist: unexpected error: $($_.Exception.Message)"
    Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
    exit 1
}
