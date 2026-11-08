$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath($PSScriptRoot)
$prefix = $root + [IO.Path]::DirectorySeparatorChar
if (!(Test-Path -LiteralPath (Join-Path $root 'Cargo.toml'))) {
    throw 'Extract the patch into the PocketHLE repository root first.'
}
foreach ($relative in (Get-Content -LiteralPath (Join-Path $root 'patch-files.txt') -Encoding UTF8)) {
    if ([string]::IsNullOrWhiteSpace($relative)) { continue }
    $path = [IO.Path]::GetFullPath((Join-Path $root $relative))
    if (!$path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Path outside repository: $relative"
    }
    (Get-Item -LiteralPath $path).LastWriteTime = Get-Date
}
Write-Host 'Add Data patch timestamps updated.'
