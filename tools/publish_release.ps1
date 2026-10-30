param(
    [Parameter(Mandatory = $true)]
    [string]$Tag,
    [string]$Title = "Aegis $Tag",
    [string]$Body = "Windows x64 prerelease of Aegis IDE and Aegis language.",
    [switch]$Build
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

if ($Tag -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+-alpha\.[0-9]+$') {
    throw "Tag must match vMAJOR.MINOR.PATCH-alpha.N"
}
if ($Build) {
    cargo build --locked --release --workspace
    Copy-Item target\release\aegis-studio.exe aegis.exe -Force
    Copy-Item target\release\aegis-cli.exe aegis-cli.exe -Force
}
$assets = @("aegis.exe", "aegis-cli.exe", "SHA256SUMS.txt", "THIRD_PARTY_NOTICES.txt")
foreach ($asset in $assets) {
    if (-not (Test-Path -LiteralPath $asset -PathType Leaf)) {
        throw "Missing release asset: $asset"
    }
}

$hashes = foreach ($asset in $assets | Where-Object { $_ -ne "SHA256SUMS.txt" }) {
    "{0}  {1}" -f (Get-FileHash -Algorithm SHA256 -LiteralPath $asset).Hash, $asset
}
Set-Content -LiteralPath SHA256SUMS.txt -Value $hashes -Encoding ascii

$credential = "protocol=https`nhost=github.com`n`n" | git credential fill
$tokenLine = $credential -split "`n" | Where-Object { $_ -like "password=*" } | Select-Object -First 1
$token = $tokenLine -replace '^password=', ''
if ([string]::IsNullOrWhiteSpace($token)) { throw "GitHub credential unavailable" }
$headers = @{
    Authorization = "Bearer $token"
    Accept = "application/vnd.github+json"
    "X-GitHub-Api-Version" = "2022-11-28"
}
$releaseBody = @{
    tag_name = $Tag
    target_commitish = "main"
    name = $Title
    body = $Body
    draft = $false
    prerelease = $true
    generate_release_notes = $false
} | ConvertTo-Json
$releaseUri = "https://api.github.com/repos/Bodyaha-Sobolev/Aegis/releases"
try {
    $release = Invoke-RestMethod -Method Get -Uri ($releaseUri + "/tags/" + $Tag) -Headers $headers
} catch {
    $release = Invoke-RestMethod -Method Post -Uri $releaseUri -Headers $headers -Body $releaseBody -ContentType "application/json"
}
$uploadBase = $release.upload_url -replace '\{\?name,label\}$', ''
foreach ($asset in $assets) {
    $contentType = if ($asset.EndsWith('.txt')) { 'text/plain' } else { 'application/octet-stream' }
    $assetHeaders = $headers.Clone()
    $assetHeaders['Content-Type'] = $contentType
    $uri = $uploadBase + '?name=' + [uri]::EscapeDataString($asset)
    try {
        Invoke-RestMethod -Method Post -Uri $uri -Headers $assetHeaders -InFile (Join-Path $root $asset) | Out-Null
    } catch {
        $existing = Invoke-RestMethod -Method Get -Uri ($release.assets_url) -Headers $headers |
            Where-Object { $_.name -eq $asset }
        if ($null -eq $existing) { throw }
        Invoke-RestMethod -Method Delete -Uri $existing.url -Headers $headers | Out-Null
        Invoke-RestMethod -Method Post -Uri $uri -Headers $assetHeaders -InFile (Join-Path $root $asset) | Out-Null
    }
}
Write-Output $release.html_url
