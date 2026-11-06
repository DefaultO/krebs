# The v8 crate's build script wants a symlink (needs Developer Mode) when target/ and the crate (vendor/v8) are on
# different drives. A directory junction needs no privileges and satisfies its "already linked" check.
# Run once per target dir: powershell -File tools\v8-junction.ps1 [targetDir]
param([string]$TargetDir = "$PSScriptRoot\..\target")
$v8 = Get-Item "$PSScriptRoot\..\vendor\v8"
foreach ($profile in 'debug', 'release') {
    $link = Join-Path $TargetDir "$profile\gn_root"
    New-Item -ItemType Directory -Force (Split-Path $link) | Out-Null
    if (Test-Path $link) { cmd /c rmdir "$link" }  # removes only the junction, never its target
    New-Item -ItemType Junction -Path $link -Target $v8.FullName | Out-Null
    Write-Output "$link -> $($v8.FullName)"
}
