#Requires -Version 5.1
<#
.SYNOPSIS
Gera o pacote .zip da extensao para envio a Chrome Web Store.
.DESCRIPTION
Copia somente os arquivos de runtime da extensao, remove o campo "key" do
manifest.json (a loja recusa pacotes com esse campo e atribui a propria chave)
e grava dist\ponte-extensao-webstore-X.Y.Z.zip com caminhos em barra normal,
como a loja exige. O manifest.json do repositorio nao e alterado.
#>
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$projectRoot = Split-Path -Parent $PSScriptRoot
$extensionDir = Join-Path $projectRoot 'extension'
$manifestPath = Join-Path $extensionDir 'manifest.json'
$utf8 = New-Object System.Text.UTF8Encoding($false)
$manifestText = [System.IO.File]::ReadAllText($manifestPath, $utf8)
$manifest = $manifestText | ConvertFrom-Json
$version = $manifest.version
if ([string]::IsNullOrWhiteSpace($version)) { throw 'manifest.json sem versao.' }

$runtimeFiles = @(
    'capture-core.js', 'files.js', 'history.js', 'content.js', 'background.js',
    'whatsapp-core.js', 'whatsapp.js',
    'popup.html', 'popup.js', 'popup.css',
    'icons/icon-16.png', 'icons/icon-32.png', 'icons/icon-48.png', 'icons/icon-128.png'
)
foreach ($relative in $runtimeFiles) {
    $full = Join-Path $extensionDir $relative
    if (-not (Test-Path -LiteralPath $full -PathType Leaf)) { throw "Arquivo da extensao ausente: $full" }
}

# Manifest for the store: identical, minus "key".
$storeManifest = $manifestText | ConvertFrom-Json
$storeManifest.PSObject.Properties.Remove('key')
$storeManifestJson = $storeManifest | ConvertTo-Json -Depth 10
# ConvertTo-Json (PowerShell 5.1) escapes a few characters as \uXXXX, which is
# valid JSON; the store reads it fine. Non-ASCII text stays as UTF-8.
if ($storeManifestJson -match '"key"') { throw 'O campo key nao foi removido.' }

$output = Join-Path $projectRoot "dist\ponte-extensao-webstore-$version.zip"
if (Test-Path -LiteralPath $output) { Remove-Item -LiteralPath $output -Force }
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$stream = [System.IO.File]::Open($output, [System.IO.FileMode]::CreateNew)
try {
    $zip = New-Object System.IO.Compression.ZipArchive($stream, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        $entry = $zip.CreateEntry('manifest.json', [System.IO.Compression.CompressionLevel]::Optimal)
        $writer = $entry.Open()
        try {
            $bytes = $utf8.GetBytes($storeManifestJson)
            $writer.Write($bytes, 0, $bytes.Length)
        } finally { $writer.Dispose() }
        foreach ($relative in $runtimeFiles) {
            [void][System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
                $zip, (Join-Path $extensionDir $relative), $relative, [System.IO.Compression.CompressionLevel]::Optimal)
        }
    } finally { $zip.Dispose() }
} finally { $stream.Dispose() }

$item = Get-Item -LiteralPath $output
Write-Host ("Pacote para a Chrome Web Store: {0} ({1:N0} bytes)" -f $item.FullName, $item.Length)
Write-Host "Versao da extensao: $version"
Write-Host 'Envie em https://chrome.google.com/webstore/devconsole (veja extension\WEBSTORE.md).'
