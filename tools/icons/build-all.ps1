<#
.SYNOPSIS
    Render every size of the release-pipeline icon.

.DESCRIPTION
    Posts each Snapshot DSL file to /snapshot and saves the PNG next to it.
    If the service requires an API key, supply it via the SNAPSHOT_API_KEY
    environment variable. Never hard-code the key in this file.

.EXAMPLE
    powershell -File build-all.ps1
    powershell -File build-all.ps1 -BaseUrl https://snapshot.example.com
#>
param(
    [string]$BaseUrl = $(if ($env:SNAPSHOT_URL) { $env:SNAPSHOT_URL } else { 'https://open-snapshot.muedsa.com' })
)

$ErrorActionPreference = 'Stop'

$sizes = @(512, 128, 64, 48, 32, 24, 16)
$failed = @()

foreach ($size in $sizes) {
    $dsl = Join-Path $PSScriptRoot "release-pipeline-$size.dsl"
    if (-not (Test-Path $dsl)) {
        Write-Host "SKIP $size : DSL not found" -ForegroundColor Yellow
        continue
    }

    $out = Join-Path $PSScriptRoot "release-pipeline-$size.png"

    $curlArgs = @(
        '-sS', '-o', $out, '-w', '%{http_code}',
        '-X', 'POST', "$($BaseUrl.TrimEnd('/'))/snapshot",
        '-H', 'Content-Type: text/plain; charset=utf-8'
    )
    if ($env:SNAPSHOT_API_KEY) {
        $curlArgs += @('-H', "X-API-Key: $env:SNAPSHOT_API_KEY")
    }
    $curlArgs += @('--data-binary', "@$dsl")

    $status = & curl.exe @curlArgs
    if ($LASTEXITCODE -ne 0 -or $status -ne '200') {
        $failed += $size
        Write-Host "FAIL $size : HTTP $status" -ForegroundColor Red
        if (Test-Path $out) { Get-Content $out -Raw }
        continue
    }

    $bytes = [System.IO.File]::ReadAllBytes($out)
    $isPng = $bytes.Length -gt 8 -and $bytes[0] -eq 0x89 -and $bytes[1] -eq 0x50
    if (-not $isPng) {
        $failed += $size
        Write-Host "FAIL $size : response body is not a PNG" -ForegroundColor Red
        Get-Content $out -Raw
        continue
    }

    Write-Host ("OK  {0,3}px  {1,6} bytes  -> {2}" -f $size, $bytes.Length, (Split-Path $out -Leaf)) -ForegroundColor Green
}

if ($failed.Count -gt 0) {
    Write-Host "`nFailed sizes: $($failed -join ', ')" -ForegroundColor Red
    exit 1
}

Write-Host "`nAll sizes rendered." -ForegroundColor Green