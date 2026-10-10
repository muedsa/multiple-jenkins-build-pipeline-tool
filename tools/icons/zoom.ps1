param(
    [Parameter(Mandatory = $true)][string]$Path,
    [int]$X = 0,
    [int]$Y = 0,
    [int]$W = 0,
    [int]$H = 0,
    [int]$Zoom = 1,
    [string]$Out
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$Path = (Resolve-Path $Path).Path
if (-not $Out) { $Out = [System.IO.Path]::ChangeExtension($Path, '.zoom.png') }

$src = [System.Drawing.Image]::FromFile($Path)
try {
    if ($W -le 0) { $W = $src.Width - $X }
    if ($H -le 0) { $H = $src.Height - $Y }
    if ($Zoom -lt 1) { $Zoom = 1 }

    $rect = New-Object System.Drawing.Rectangle $X, $Y, $W, $H
    $bmp = New-Object System.Drawing.Bitmap ($W * $Zoom), ($H * $Zoom)
    try {
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        try {
            $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
            $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
            # 棋盘格底，便于观察透明区域
            $g.Clear([System.Drawing.Color]::FromArgb(255, 40, 40, 40))
            $brush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 90, 90, 90))
            for ($ty = 0; $ty -lt $H; $ty += 8) {
                for ($tx = 0; $tx -lt $W; $tx += 8) {
                    if ((([int]($tx / 8) + [int]($ty / 8)) % 2) -eq 0) {
                        $g.FillRectangle($brush, $tx * $Zoom, $ty * $Zoom, 8 * $Zoom, 8 * $Zoom)
                    }
                }
            }
            $brush.Dispose()
            $g.DrawImage($src, (New-Object System.Drawing.Rectangle 0, 0, ($W * $Zoom), ($H * $Zoom)), $rect, [System.Drawing.GraphicsUnit]::Pixel)
        }
        finally { $g.Dispose() }
        $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
    }
    finally { $bmp.Dispose() }
}
finally { $src.Dispose() }

Write-Host "OK $Out"