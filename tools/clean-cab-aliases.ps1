# Preview: .\clean-cab-aliases.ps1 -LibraryRoot "$env:USERPROFILE\Documents\PocketHLE"
# Apply:   .\clean-cab-aliases.ps1 -LibraryRoot "$env:USERPROFILE\Documents\PocketHLE" -Apply
[CmdletBinding(SupportsShouldProcess)]
param(
    [string]$LibraryRoot = (Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'PocketHLE'),
    [switch]$Apply
)

$ErrorActionPreference = 'Stop'
$root = (Get-Item -LiteralPath $LibraryRoot).FullName
$games = Get-Item -LiteralPath (Join-Path $root 'games')
$reparse = [IO.FileAttributes]::ReparsePoint
if ($games.Attributes -band $reparse) { throw 'The games directory is a link.' }
$backup = Join-Path $root ('cab-alias-backup-' + [Guid]::NewGuid().ToString('N'))
$count = 0
$bytes = 0L

# Read the FILES table of a legacy Windows CE MSCE manifest. Resolve
# numbered members by ID.
function Get-MsceAliases($Manifest, $Members) {
    if ($Manifest.Length -lt 100 -or $Manifest.Length -gt 16MB) { return }
    $data = [IO.File]::ReadAllBytes($Manifest.FullName)
    if ([Text.Encoding]::ASCII.GetString($data, 0, 4) -cne 'MSCE') { return }
    $count = [BitConverter]::ToUInt16($data, 52)
    $offset = [long][BitConverter]::ToUInt32($data, 68)
    $end = [long][BitConverter]::ToUInt32($data, 72)
    if ($offset -lt 100 -or $end -lt $offset -or $end -gt $data.Length) { return }
    $records = @()
    $utf8 = New-Object System.Text.UTF8Encoding -ArgumentList $false, $true
    for ($index = 0; $index -lt $count; $index++) {
        if ($offset + 12 -gt $end) { return }
        $id = [BitConverter]::ToUInt16($data, $offset)
        $length = [BitConverter]::ToUInt16($data, ($offset + 10))
        $offset += 12
        if ($length -lt 2 -or $offset + $length -gt $end) { return }
        $terminator = [Array]::IndexOf($data, [byte]0, [int]$offset, [int]$length)
        if ($terminator -le $offset) { return }
        $nameLength = $terminator - $offset
        try {
            $name = $utf8.GetString($data, $offset, $nameLength)
        } catch [System.Text.DecoderFallbackException] {
            $name = [Text.Encoding]::GetEncoding(1252).GetString($data, $offset, $nameLength)
        }
        $offset += $length
        $ext = '.{0:D3}' -f $id
        $sources = @($Members | Where-Object { $_.Extension -eq $ext })
        if ($sources.Count -eq 1) {
            $records += [pscustomobject]@{ Source = $sources[0].Name; Target = $name }
        } else {
            $records += [pscustomobject]@{ Source = $ext; Target = $name }
        }
    }
    $records
}

foreach ($game in Get-ChildItem -LiteralPath $games.FullName -Directory) {
    if ($game.Attributes -band $reparse) { continue }
    $extracted = Get-Item -LiteralPath (Join-Path $game.FullName 'extracted') -ErrorAction SilentlyContinue
    if (!$extracted -or !$extracted.PSIsContainer -or ($extracted.Attributes -band $reparse)) { continue }
    $metadata = Get-Item -LiteralPath (Join-Path $game.FullName 'game.json') -ErrorAction SilentlyContinue
    $setup = Get-Item -LiteralPath (Join-Path $extracted.FullName '_setup.xml') -ErrorAction SilentlyContinue
    if (!$metadata -or ($metadata.Attributes -band $reparse)) { continue }
    $members = @(Get-ChildItem -LiteralPath $extracted.FullName -File)
    $headers = @($members | Where-Object { $_.Extension -eq ".000" })

    try {
        $entry = Get-Content -LiteralPath $metadata.FullName -Raw | ConvertFrom-Json
        if (!$entry.executable) { continue }
        $exe = [IO.Path]::GetFullPath((Join-Path $game.FullName $entry.executable))
        $records = @()
        if ($setup -and !($setup.Attributes -band $reparse)) {
            $xml = New-Object System.Xml.XmlDocument
            $xml.XmlResolver = $null
            $xml.Load($setup.FullName)
            foreach ($node in $xml.SelectNodes("//characteristic[@type='Extract']/parm[@name='Source']")) {
                $records += [pscustomobject]@{
                    Source = $node.GetAttribute('value')
                    Target = $node.ParentNode.ParentNode.GetAttribute('type')
                }
            }
        }
        if ($headers.Count -eq 1 -and !($headers[0].Attributes -band $reparse)) {
            $records += @(Get-MsceAliases $headers[0] $members)
        }
    } catch {
        Write-Warning "Skipping $($game.Name): $_"
        continue
    }

    $installedByName = @{}
    $dirs = [Collections.Generic.Stack[string]]::new()
    $dirs.Push($extracted.FullName)
    while ($dirs.Count -gt 0) {
        foreach ($item in Get-ChildItem -LiteralPath ($dirs.Pop())) {
            if ($item.Attributes -band $reparse) { continue }
            if ($item.PSIsContainer) {
                $dirs.Push($item.FullName)
            } else {
                $installedByName[$item.Name] = @($installedByName[$item.Name] | Where-Object { $_ }) + $item
            }
        }
    }

    $destinations = @{}
    $bySource = @{}
    foreach ($record in $records) {
        $source = $record.Source
        $name = $record.Target
        if (!$name -or $name.Contains([char]0)) { continue }
        $destinations[$name] = @($destinations[$name] | Where-Object { $_ }) + $source
        if ($source -match '^[^\\/:]+\.(?!000$|999$)[0-9]{3}$') {
            $bySource[$source] = @($bySource[$source] | Where-Object { $_ }) + $name
        }
    }

    # When a basename occurs in several directories, require every copy to
    # match because this script does not resolve Windows CE install paths.
    foreach ($name in $bySource.Keys) {
        if ($destinations.ContainsKey($name)) { continue }
        $source = Get-Item -LiteralPath (Join-Path $extracted.FullName $name) -ErrorAction SilentlyContinue
        if (!$source -or $source.PSIsContainer -or ($source.Attributes -band $reparse) -or $source.FullName -eq $exe) { continue }
        $sourceHash = $null
        $verified = $true
        foreach ($target in ($bySource[$name] | Sort-Object -Unique)) {
            if (!$target -or $target -match '[\\/:]' -or $target -in @('.', '..') -or $target -eq $name) {
                $verified = $false
                break
            }
            $matchesForTarget = @($installedByName[$target] | Where-Object { $_ })
            if (!$matchesForTarget.Count) {
                $verified = $false
                break
            }
            if (!$sourceHash) { $sourceHash = (Get-FileHash -LiteralPath $source.FullName -Algorithm SHA256).Hash }
            foreach ($copy in $matchesForTarget) {
                if ($copy.Length -ne $source.Length -or (Get-FileHash -LiteralPath $copy.FullName -Algorithm SHA256).Hash -ne $sourceHash) {
                    $verified = $false
                    break
                }
            }
            if (!$verified) { break }
        }
        if (!$verified) { continue }
        $count++
        $bytes += $source.Length
        if (!$Apply) {
            Write-Output $source.FullName
        } elseif ($PSCmdlet.ShouldProcess($source.FullName, 'Move redundant CAB member to backup')) {
            $folder = Join-Path (Join-Path $backup $game.Name) 'extracted'
            [IO.Directory]::CreateDirectory($folder) | Out-Null
            Move-Item -LiteralPath $source.FullName -Destination (Join-Path $folder $source.Name)
        }
    }
}

Write-Host ('{0} verified aliases, {1:N1} MiB.' -f $count, ($bytes / 1MB))
if ($Apply -and (Test-Path -LiteralPath $backup)) { Write-Host "Backup: $backup" }
