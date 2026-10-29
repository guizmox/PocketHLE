# Windows PowerShell 5.1. Creates the PUBLIC RELEASE key outside the repository.
$ErrorActionPreference = 'Stop'
try {
    Write-Host @'
PocketHLE - Set up permanent Android release signing
===================================================
Run this ONCE, on your own Windows computer. No Python or Android SDK required.
Your personal WSL debug key is NOT touched.
Keep a separate private backup of the generated folder: future updates need it.
Never commit the key or the secret text files to Git, or share them publicly.
'@
    $folder = Join-Path $env:USERPROFILE '.pockethle\android-signing'
    $keystore = Join-Path $folder 'pockethle-release.jks'
    $names = @('POCKETHLE_KEYSTORE_BASE64', 'POCKETHLE_KEYSTORE_PASSWORD', 'POCKETHLE_KEY_ALIAS', 'POCKETHLE_KEY_PASSWORD')
    $complete = (Test-Path -LiteralPath $keystore)
    foreach ($name in $names) { $complete = $complete -and (Test-Path -LiteralPath (Join-Path $folder ($name + '.txt'))) }
    if (!$complete) {
        if (Test-Path -LiteralPath $folder) {
            throw "The signing folder already exists but is incomplete: $folder. Nothing was overwritten. Keep its contents and recover the existing key before continuing."
        }
        $candidates = @()
        $onPath = Get-Command keytool.exe -ErrorAction SilentlyContinue
        if ($onPath) { $candidates += $onPath.Source }
        if ($env:JAVA_HOME) { $candidates += (Join-Path $env:JAVA_HOME 'bin\keytool.exe') }
        if ($env:ProgramFiles) { $candidates += (Join-Path $env:ProgramFiles 'Android\Android Studio\jbr\bin\keytool.exe') }
        $keytool = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
        $wsl = $null
        if (!$keytool) {
            $wslCommand = Get-Command wsl.exe -ErrorAction SilentlyContinue
            if ($wslCommand) {
                Write-Host 'Windows keytool not found. Checking Java in your default WSL distribution...'
                & $wslCommand.Source --exec bash -lc 'command -v keytool >/dev/null 2>&1'
                if ($LASTEXITCODE -eq 0) { $wsl = $wslCommand.Source }
            }
            if (!$wsl) {
                Write-Host 'No keytool found in Windows or the default WSL distribution.'
                Write-Host 'In the WSL distribution you use for Android builds, check: command -v keytool'
                Write-Host 'Alternatively install a Windows JDK: https://learn.microsoft.com/java/openjdk/download'
                throw 'Java keytool is required. No key was created.'
            }
            Write-Host 'Using WSL Java. The release key will still be saved in your private WINDOWS folder.'
        }
        New-Item -ItemType Directory -Path $folder | Out-Null
        # Restrict this folder to the current Windows account before writing secrets.
        $acl = New-Object System.Security.AccessControl.DirectorySecurity
        $sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
        $acl.SetOwner($sid)
        $acl.SetAccessRuleProtection($true, $false)
        $rule = New-Object System.Security.AccessControl.FileSystemAccessRule($sid, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
        $acl.AddAccessRule($rule)
        Set-Acl -LiteralPath $folder -AclObject $acl
        $bytes = New-Object byte[] 32
        $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
        try { $rng.GetBytes($bytes) } finally { $rng.Dispose() }
        $password = [Convert]::ToBase64String($bytes)
        $alias = 'pockethle-release'
        $previous = $env:POCKETHLE_SETUP_PASSWORD
        $env:POCKETHLE_SETUP_PASSWORD = $password
        try {
            Write-Host 'Creating your permanent release key...'
            if ($keytool) {
                & $keytool -genkeypair -noprompt -keystore $keystore -storetype JKS -alias $alias -keyalg RSA -keysize 3072 -validity 10000 -dname 'CN=PocketHLE Release' -storepass:env POCKETHLE_SETUP_PASSWORD -keypass:env POCKETHLE_SETUP_PASSWORD
                if ($LASTEXITCODE -ne 0) { throw 'keytool failed. Keep the signing folder for inspection; no existing key was replaced.' }
                & $keytool -list -keystore $keystore -alias $alias -storepass:env POCKETHLE_SETUP_PASSWORD
                if ($LASTEXITCODE -ne 0) { throw 'Created key could not be verified.' }
            } else {
                # Send paths and password through stdin, never through command-line arguments.
                # Base64 protects spaces, quotes and non-ASCII Windows profile paths.
                $pathBase64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($keystore))
                $bash = @'
set -euo pipefail
windows_path=$(printf '%s' '__PATH_BASE64__' | base64 --decode)
keystore=$(wslpath -u "$windows_path")
export POCKETHLE_SETUP_PASSWORD='__PASSWORD__'
trap 'unset POCKETHLE_SETUP_PASSWORD' EXIT
if [[ -e "$keystore" ]]; then
    echo 'Refusing to overwrite an existing release key.' >&2
    exit 1
fi
keytool -genkeypair -noprompt -keystore "$keystore" -storetype JKS -alias pockethle-release -keyalg RSA -keysize 3072 -validity 10000 -dname 'CN=PocketHLE Release' -storepass:env POCKETHLE_SETUP_PASSWORD -keypass:env POCKETHLE_SETUP_PASSWORD
keytool -list -keystore "$keystore" -alias pockethle-release -storepass:env POCKETHLE_SETUP_PASSWORD
'@
                $bash = $bash.Replace('__PATH_BASE64__', $pathBase64).Replace('__PASSWORD__', $password)
                $start = New-Object Diagnostics.ProcessStartInfo
                $start.FileName = $wsl
                $start.Arguments = '--exec bash -l -s'
                $start.UseShellExecute = $false
                $start.RedirectStandardInput = $true
                $process = [Diagnostics.Process]::Start($start)
                try {
                    $process.StandardInput.Write($bash.Replace("`r`n", "`n") + "`n")
                    $process.StandardInput.Close()
                    $process.WaitForExit()
                    if ($process.ExitCode -ne 0) { throw 'WSL keytool failed. Keep the signing folder for inspection; no existing key was replaced.' }
                } finally { $process.Dispose(); $bash = $null }
                if (!(Test-Path -LiteralPath $keystore)) { throw 'WSL did not create the Windows release key.' }
            }
        } finally { $env:POCKETHLE_SETUP_PASSWORD = $previous }
        $values = @([Convert]::ToBase64String([IO.File]::ReadAllBytes($keystore)), $password, $alias, $password)
        for ($i = 0; $i -lt $names.Count; $i++) {
            [IO.File]::WriteAllText((Join-Path $folder ($names[$i] + '.txt')), $values[$i], (New-Object Text.UTF8Encoding($false)))
        }
        $password = $null
        $values = $null
        Write-Host 'Key created and verified. This key will be reused for every public release.'
    } else {
        Write-Host 'Existing key found. Reusing it; no key or secret was replaced.'
    }
    Write-Host "PRIVATE BACKUP FOLDER: $folder"
    Write-Host 'Copy this folder to a safe private backup location before publishing.'
    Write-Host ''
    Write-Host 'Open: https://github.com/guizmox/PocketHLE/settings/secrets/actions'
    Write-Host 'For each item below, click New repository secret (or update the existing secret).'
    Write-Host 'Paste the displayed NAME into Name and the clipboard content into Secret, then save.'
    foreach ($name in $names) {
        Write-Host ''
        Write-Host "NAME: $name"
        Read-Host 'Press Enter to copy its VALUE to the clipboard' | Out-Null
        Set-Clipboard -Value ([IO.File]::ReadAllText((Join-Path $folder ($name + '.txt'))))
        Read-Host 'Value copied. Paste it into GitHub, save the secret, then press Enter here' | Out-Null
    }
    Set-Clipboard -Value ''
    Write-Host ''
    Write-Host 'All four values are prepared. GitHub configuration is complete only after you save all four secrets.'
    Write-Host 'Now rerun the GitHub CI workflow and replace the public Android release ZIP with the signed artifact.'
    Write-Host 'The APK filename must NOT contain unsigned. Keep this key for all future releases.'
    Write-Host 'Existing installs with a different old CI key need a backup before uninstall/reinstall.'
} catch {
    Write-Error $_.Exception.Message -ErrorAction Continue
    exit 1
}
