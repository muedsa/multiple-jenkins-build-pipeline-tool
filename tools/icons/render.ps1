param(
    [Parameter(Mandatory = $true)][string]$Dsl,
    [string]$Out
)

$ErrorActionPreference = 'Stop'

$Dsl = (Resolve-Path $Dsl).Path
if (-not $Out) {
    $Out = [System.IO.Path]::ChangeExtension($Dsl, '.png')
}
elseif (-not [System.IO.Path]::IsPathRooted($Out)) {
    $Out = Join-Path (Get-Location).Path $Out
}

$base = if ($env:SNAPSHOT_URL) { $env:SNAPSHOT_URL } else { 'https://open-snapshot.muedsa.com' }
$base = $base.TrimEnd('/')

$curlArgs = @(
    '-sS', '-o', $Out, '-w', '%{http_code}',
    '-X', 'POST', "$base/snapshot",
    '-H', 'Content-Type: text/plain; charset=utf-8'
)
if ($env:SNAPSHOT_API_KEY) {
    $curlArgs += @('-H', "X-API-Key: $env:SNAPSHOT_API_KEY")
}
$curlArgs += @('--data-binary', "@$Dsl")

$status = & curl.exe @curlArgs
if ($LASTEXITCODE -ne 0) { throw "curl failed with exit code $LASTEXITCODE" }

if ($status -ne '200') {
    Write-Host "FAILED $status -> $Out" -ForegroundColor Red
    Get-Content $Out -Raw
    exit 1
}

# 校验响应体确实是图片，而不是被写进文件的 JSON 错误
$bytes = [System.IO.File]::ReadAllBytes($Out)
$isPng = $bytes.Length -gt 8 -and $bytes[0] -eq 0x89 -and $bytes[1] -eq 0x50 -and $bytes[2] -eq 0x4E -and $bytes[3] -eq 0x47
if (-not $isPng) {
    Write-Host "FAILED: response is not a PNG ($($bytes.Length) bytes)" -ForegroundColor Red
    Get-Content $Out -Raw
    exit 1
}

Write-Host ("OK {0}  {1} bytes  -> {2}" -f $status, $bytes.Length, $Out) -ForegroundColor Green