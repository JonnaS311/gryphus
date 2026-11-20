# Mide el arranque en frío de la app de release sin WebDriver (como la abre un usuario).
# Uso: powershell -File tools/measure-startup.ps1 [-Runs 10] [-Exe ruta\gryphus.exe]
# Requiere un binario con el frontend embebido:
#   trunk --config ui/Trunk.toml build --release
#   cargo build -p gryphus-desktop --release --features custom-protocol --target-dir target/e2e-release
param(
    [int]$Runs = 10,
    [string]$Exe = "$PSScriptRoot\..\target\e2e-release\release\gryphus.exe"
)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path $Exe).Path
$work = Join-Path ([IO.Path]::GetTempPath()) ("gryphus-startup-" + [guid]::NewGuid())
New-Item -ItemType Directory $work | Out-Null
$env:GRYPHUS_DATA_DIR = $work
$env:GRYPHUS_STARTUP_EXIT = '1'
$results = @()
try {
    for ($i = 1; $i -le $Runs; $i++) {
        $log = Join-Path $work "run$i.log"
        $env:GRYPHUS_STARTUP_LOG = $log
        $p = Start-Process $exe -PassThru
        if (-not $p.WaitForExit(15000)) { $p.Kill(); throw "la app no terminó (run $i)" }
        $row = [ordered]@{ run = $i }
        Get-Content $log | ForEach-Object { $k, $v = $_ -split ' '; $row[$k] = [int]$v }
        $results += [pscustomobject]$row
    }
} finally {
    Remove-Item Env:GRYPHUS_STARTUP_EXIT, Env:GRYPHUS_STARTUP_LOG, Env:GRYPHUS_DATA_DIR -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
$results | Format-Table -AutoSize
foreach ($phase in 'setup', 'page_started', 'ui_ready', 'page_finished') {
    $vals = $results | ForEach-Object { $_.$phase } | Where-Object { $_ -ne $null } | Sort-Object
    if ($vals) { "{0,-14} mediana {1,5} ms  (min {2}, max {3})" -f $phase, $vals[[int][math]::Floor($vals.Count / 2)], $vals[0], $vals[-1] }
}
