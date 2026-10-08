param(
    [string]$MsysRoot = 'C:\msys64',
    [int]$Jobs = 4
)
$ErrorActionPreference = 'Stop'
if ($Jobs -lt 1) { throw 'Jobs must be positive.' }
$projectRoot = Split-Path $PSScriptRoot -Parent
$bash = Join-Path $MsysRoot 'usr\bin\bash.exe'
if (!(Test-Path $bash)) {
    throw "MSYS2 is required only to build FFmpeg. Install it at https://www.msys2.org/, run 'pacman -S --needed make diffutils', then run this script again. Custom location: -MsysRoot D:\msys64"
}
if (!(Get-Command cl.exe -ErrorAction SilentlyContinue) -or $env:VSCMD_ARG_TGT_ARCH -ne 'x64') {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (!(Test-Path $vswhere)) { throw 'Visual Studio C++ Build Tools are required (x64, VS 2019 16.8 or newer).' }
    $visualStudio = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (!$visualStudio) { throw 'No Visual Studio C++ toolchain found.' }
    $vcvars = Join-Path $visualStudio 'VC\Auxiliary\Build\vcvars64.bat'
    $lines = & $env:ComSpec /d /s /c "`"`"$vcvars`" >nul && set`""
    if ($LASTEXITCODE -ne 0) { throw 'Could not initialize the Visual Studio x64 environment.' }
    foreach ($line in $lines) {
        $separator = $line.IndexOf('=')
        if ($separator -gt 0) { [Environment]::SetEnvironmentVariable($line.Substring(0,$separator),$line.Substring($separator+1),'Process') }
    }
}
# Non-login Bash preserves PATH/INCLUDE/LIB from vcvars. Do not substitute a
# MinGW compiler: the output must match Rust's x86_64-pc-windows-msvc ABI.
$previousPath = $env:PATH
try {
    $env:PATH = (Join-Path $MsysRoot 'usr\bin') + ';' + $env:PATH
    & $bash (Join-Path $PSScriptRoot 'build-ffmpeg.sh') 'x86_64-pc-windows-msvc' $Jobs
    if ($LASTEXITCODE -ne 0) { throw 'Static FFmpeg build failed. See the configure/build output above.' }
} finally {
    $env:PATH = $previousPath
}
Write-Host 'Ready: cargo build --release -p pocket-desktop'
