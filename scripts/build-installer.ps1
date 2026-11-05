#Requires -Version 5.1
<#
.SYNOPSIS
Gera dist\Ponte-Setup-X.Y.Z.exe com o Inno Setup a partir de dist\ponteproj.exe.
.DESCRIPTION
Le a versao em Cargo.toml, confere o executavel em dist, inclui o ID da extensao
publicada na Chrome Web Store quando extension-store-id.txt existir e compila
installer\Ponte.iss. Requer o Inno Setup 6 (winget install JRSoftware.InnoSetup).
.EXAMPLE
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-installer.ps1
#>
[CmdletBinding()]
param(
    [string]$InnoCompiler
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$projectRoot = Split-Path -Parent $PSScriptRoot
$cargo = Get-Content -LiteralPath (Join-Path $projectRoot 'Cargo.toml') -Raw
if ($cargo -notmatch '(?m)^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"') {
    throw 'Versao nao encontrada em Cargo.toml.'
}
$version = $Matches[1]

$executable = Join-Path $projectRoot 'dist\ponteproj.exe'
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw "dist\ponteproj.exe nao encontrado. Gere o release antes (scripts\build.ps1 -Action Release) e copie para dist."
}
$script = Join-Path $projectRoot 'installer\Ponte.iss'
if (-not (Test-Path -LiteralPath $script -PathType Leaf)) {
    throw "Script do instalador nao encontrado: $script"
}

# The extension published in the Chrome Web Store has an ID of its own; once
# known, it is recorded in extension-store-id.txt and authorized as well.
$storeId = ''
$storeIdPath = Join-Path $projectRoot 'extension-store-id.txt'
if (Test-Path -LiteralPath $storeIdPath -PathType Leaf) {
    $candidate = (Get-Content -LiteralPath $storeIdPath -Raw).Trim()
    if ($candidate -cmatch '^[a-p]{32}$') {
        $storeId = $candidate
    } elseif ($candidate -ne '') {
        throw "extension-store-id.txt deve conter apenas o ID de 32 letras (a-p) da extensao na loja."
    }
}

if ([string]::IsNullOrWhiteSpace($InnoCompiler)) {
    $candidates = @(
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
        (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe')
    )
    $InnoCompiler = $candidates | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) } | Select-Object -First 1
    if ([string]::IsNullOrWhiteSpace($InnoCompiler)) {
        throw 'ISCC.exe (Inno Setup 6) nao encontrado. Instale com: winget install JRSoftware.InnoSetup'
    }
}

$arguments = @("/DAppVersion=$version")
if ($storeId) { $arguments += "/DStoreExtensionId=$storeId" }
$arguments += $script

Write-Host "Versao: $version"
Write-Host "Executavel: $executable"
Write-Host ("Extensao da loja: " + $(if ($storeId) { $storeId } else { 'ainda nao publicada (somente a extensao carregada da pasta)' }))
Write-Host "Compilador: $InnoCompiler"

& $InnoCompiler @arguments
if ($LASTEXITCODE -ne 0) {
    throw "O Inno Setup terminou com codigo $LASTEXITCODE."
}
$output = Join-Path $projectRoot "dist\Ponte-Setup-$version.exe"
if (-not (Test-Path -LiteralPath $output -PathType Leaf)) {
    throw "Instalador nao gerado em $output."
}
$item = Get-Item -LiteralPath $output
Write-Host ("Instalador: {0} ({1:N0} bytes)" -f $item.FullName, $item.Length)
Write-Host ("SHA-256: " + (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash)
# Copy with a fixed name: published beside the versioned one, it is what the
# permanent link releases/latest/download/Ponte-Setup.exe downloads.
$fixed = Join-Path $projectRoot 'dist\Ponte-Setup.exe'
Copy-Item -LiteralPath $output -Destination $fixed -Force
Write-Host "Copia de nome fixo (link direto da Release mais recente): $fixed"
