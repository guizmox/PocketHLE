# Invoked by backup-android-adb.bat; requires Windows PowerShell 5.1 and adb.
param(
    [string]$Output = ("PocketHLE-backup-{0}.zip" -f (Get-Date -Format 'yyyyMMdd-HHmmss')),
    [string]$Serial = ''
)
$ErrorActionPreference = 'Stop'
$env:PATH = (Join-Path $PSScriptRoot 'platform-tools') + ';' + $PSScriptRoot + ';' + $env:PATH
$scratch = $null
function Invoke-Adb([string[]]$Arguments) {
    & adb @script:deviceArgs @Arguments
    if ($LASTEXITCODE -ne 0) { throw "ADB failed: $($Arguments -join ' ')" }
}
function Export-Tar([string]$Source, [string]$Destination) {
    # Binary stdout must not pass through PowerShell 5's text redirection.
    $start = New-Object System.Diagnostics.ProcessStartInfo
    $start.FileName = (Get-Command adb -ErrorAction Stop).Source
    $argsList = @($script:deviceArgs) + @('exec-out','run-as','com.pockethle.app','tar','-C',$Source,'-cf','-','.')
    $start.Arguments = ($argsList | ForEach-Object { '"' + $_ + '"' }) -join ' '
    $start.UseShellExecute = $false
    $start.RedirectStandardOutput = $true
    $process = [System.Diagnostics.Process]::Start($start)
    try {
        $stream = [IO.File]::Create($Destination)
        try { $process.StandardOutput.BaseStream.CopyTo($stream) } finally { $stream.Dispose() }
        $process.WaitForExit()
        return ($process.ExitCode -eq 0)
    } finally { $process.Dispose() }
}
try {
    Write-Host @'
PocketHLE - Android library rescue backup
----------------------------------------
This saves your games, settings and saves to a ZIP on this computer.
It NEVER uninstalls PocketHLE or modifies its library.

Before starting:
  - Install Android Platform Tools and add adb.exe to PATH.
  - Windows PowerShell 5.1 is included with Windows 10/11.
    tar.exe (included with recent Windows 10/11) is needed only for fallback.
  - Enable USB debugging on the Android device and connect it by USB.
  - Unlock it and accept the USB debugging authorization prompt.
  - Close any running game/import and leave enough disk space for TWO copies
    of your library (temporary files plus the finished ZIP).
  - With several devices connected, provide the device serial as argument 2.


ADB download (official, Windows/Linux):
  https://developer.android.com/tools/releases/platform-tools
Extract the download, then put the platform-tools folder beside this script,
or add its full path to PATH. Android Studio is NOT required.

To enable ADB / USB debugging on the phone or handheld:
  1. Open Settings > About phone (or About device).
  2. Tap Build number seven times; enter your PIN if asked.
     Samsung: About phone > Software information > Build number.
  3. Open Settings > System > Developer options (location varies by device).
  4. Enable USB debugging. Bootloader unlocking / OEM unlocking is NOT needed.
  5. Connect a USB data cable, unlock the device, and accept Allow USB debugging.
  6. If ADB says unauthorized, check that authorization prompt on the device.
Official guide: https://developer.android.com/studio/debug/dev-options

Do NOT uninstall PocketHLE until this script reports "Verified backup".
'@
    Get-Command adb -ErrorAction Stop | Out-Null
    if ($Serial -and $Serial -notmatch '^[a-zA-Z0-9_.:-]+$') { throw 'Invalid device serial.' }
    $script:deviceArgs = @()
    if ($Serial) { $script:deviceArgs = @('-s', $Serial) }
    $Output = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Output)
    if (Test-Path -LiteralPath $Output) { throw 'Output already exists; choose another ZIP name.' }
    $parent = Split-Path -Parent $Output
    $scratch = Join-Path $parent ('.pockethle-rescue-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $scratch | Out-Null
    Write-Host '[1/4] Checking the ADB connection...'
    Invoke-Adb -Arguments @('get-state')
    Write-Host '[2/4] Stopping PocketHLE and copying its library (large libraries may take a while)...'
    Invoke-Adb -Arguments @('shell','am','force-stop','com.pockethle.app')
    $library = Join-Path $scratch 'library'
    $external = '/sdcard/Android/data/com.pockethle.app/files/library'
    & adb @script:deviceArgs pull $external $library
    if ($LASTEXITCODE -ne 0 -or !(Test-Path -LiteralPath (Join-Path $library 'library.json'))) {
        Write-Host 'Direct access unavailable. Trying the installed debug app (run-as)...'
        if (Test-Path -LiteralPath $library) { Remove-Item -LiteralPath $library -Recurse -Force }
        Get-Command tar.exe -ErrorAction Stop | Out-Null
        foreach ($source in @($external, '/data/user/0/com.pockethle.app/files/library')) {
            $tar = Join-Path $scratch 'rescue.tar'
            if (!(Export-Tar $source $tar)) { continue }
            $names = @(& tar.exe -tf $tar)
            if ($LASTEXITCODE -ne 0) { continue }
            $types = @(& tar.exe -tvf $tar)
            if ($LASTEXITCODE -ne 0) { continue }
            foreach ($line in $types) {
                if ($line -notmatch '^[-d]') { throw 'Unsafe TAR entry.' }
            }
            foreach ($name in $names) {
                if ($name -match '^/|\\|:|(^|/)\.\.(/|$)') { throw 'Unsafe TAR path.' }
            }
            New-Item -ItemType Directory -Path $library | Out-Null
            & tar.exe -xf $tar -C $library
            if ($LASTEXITCODE -eq 0 -and (Test-Path -LiteralPath (Join-Path $library 'library.json'))) { break }
            Remove-Item -LiteralPath $library -Recurse -Force
        }
    }
    $catalog = Join-Path $library 'library.json'
    if (!(Test-Path -LiteralPath $catalog)) { throw 'Android refused access; no usable backup created.' }
    $value = Get-Content -LiteralPath $catalog -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($null -eq $value -or $null -eq $value.PSObject.Properties['games'] -or $value.games -isnot [Array]) {
        throw 'Invalid library.json.'
    }
    $items = @(Get-ChildItem -LiteralPath $library -Recurse -Force)
    foreach ($item in $items) {
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Links are unsupported.' }
    }
    Write-Host '[3/4] Creating the restore ZIP...'
    [IO.File]::WriteAllText((Join-Path $library '__pockethle_backup__.json'), '{"format":"PocketHLE","version":1}', (New-Object Text.UTF8Encoding($false)))
    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $staged = Join-Path $scratch 'backup.zip'
    $zip = [IO.Compression.ZipFile]::Open($staged, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($file in Get-ChildItem -LiteralPath $library -Recurse -Force -File) {
            $relative = $file.FullName.Substring($library.Length + 1).Replace('\','/')
            [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $file.FullName, $relative, [IO.Compression.CompressionLevel]::Optimal) | Out-Null
        }
    } finally { $zip.Dispose() }
    Write-Host '[4/4] Verifying every ZIP entry...'
    # Read every ZIP entry and compare its SHA256 with the copied source bytes.
    $zip = [IO.Compression.ZipFile]::OpenRead($staged)
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        foreach ($entry in $zip.Entries) {
            $stream = $entry.Open()
            try { $actual = [BitConverter]::ToString($sha.ComputeHash($stream)) } finally { $stream.Dispose() }
            $file = Join-Path $library ($entry.FullName.Replace('/','\'))
            $stream = [IO.File]::OpenRead($file)
            try { $expected = [BitConverter]::ToString($sha.ComputeHash($stream)) } finally { $stream.Dispose() }
            if ($actual -ne $expected) { throw "ZIP verification failed: $($entry.FullName)" }
        }
    } finally { $sha.Dispose(); $zip.Dispose() }
    [IO.File]::Move($staged, $Output) # Refuses to overwrite an existing destination.
    Write-Host "Verified backup: $Output"
    Write-Host 'Keep a separate copy of this ZIP before uninstalling anything.'
    Write-Host 'After installing the new APK, use PocketHLE: Restore library backup (ZIP).'
} catch {
    Write-Error "$($_.Exception.Message) KEEP THE APP INSTALLED." -ErrorAction Continue
    exit 1
} finally {
    if ($scratch -and (Test-Path -LiteralPath $scratch)) { Remove-Item -LiteralPath $scratch -Recurse -Force }
}
