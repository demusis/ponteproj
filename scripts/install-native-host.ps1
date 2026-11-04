#Requires -Version 5.1
<#
.SYNOPSIS
Registra a ponte nativa PonteProj para o usuario atual do Chrome e do Edge.
.DESCRIPTION
Grava somente o manifesto do proprio host em LOCALAPPDATA/PonteProj e o valor
padrao das duas chaves HKCU NativeMessagingHosts/com.ponteproj.assistente.
Nao instala a extensao nem altera perfis, politicas ou preferencias do navegador.
Use -WhatIf para validar e inspecionar o destino sem gravar.
.PARAMETER Executable
Caminho do ponteproj.exe. Sem este parametro, procura dist, target/release e
target/debug, nessa ordem, em relacao a raiz do projeto.
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [string]$Executable
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($env:OS -ne 'Windows_NT') {
    throw 'Esta ponte nativa requer Windows.'
}
if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
    throw 'LOCALAPPDATA indisponivel para o usuario atual.'
}

$projectRoot = Split-Path -Parent $PSScriptRoot
$hostName = 'com.ponteproj.assistente'
$expectedExtensionId = 'ecllnakmppjinmednoikimbpnbgbimhg'
$extensionIdPath = Join-Path $projectRoot 'extension-id.txt'
$extensionManifestPath = Join-Path $projectRoot 'extension/manifest.json'
$extensionKeyPath = Join-Path $projectRoot 'extension-key.txt'

foreach ($requiredPath in @($extensionIdPath, $extensionManifestPath, $extensionKeyPath)) {
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
        throw "Arquivo do pacote nao encontrado: $requiredPath"
    }
}

$extensionId = (Get-Content -LiteralPath $extensionIdPath -Raw).Trim()
if ($extensionId -cnotmatch '^[a-p]{32}$' -or $extensionId -cne $expectedExtensionId) {
    throw 'extension-id.txt nao contem o ID fixo esperado para este pacote.'
}
$extensionManifest = Get-Content -LiteralPath $extensionManifestPath -Raw | ConvertFrom-Json
$publicKey = (Get-Content -LiteralPath $extensionKeyPath -Raw).Trim()
if ([string]::IsNullOrWhiteSpace($extensionManifest.key) -or $extensionManifest.key -cne $publicKey) {
    throw 'A chave publica da extensao difere de extension-key.txt. Preserve manifest.key e o ID do pacote.'
}

# Chrome derives the extension ID from the first 128 bits of SHA256(public key).
$hasher = [System.Security.Cryptography.SHA256]::Create()
try {
    $keyBytes = [Convert]::FromBase64String($publicKey)
    $keyHash = $hasher.ComputeHash($keyBytes)
} finally {
    $hasher.Dispose()
}
$derivedId = [System.Text.StringBuilder]::new(32)
for ($index = 0; $index -lt 16; $index++) {
    [void]$derivedId.Append([char](97 + ($keyHash[$index] -shr 4)))
    [void]$derivedId.Append([char](97 + ($keyHash[$index] -band 15)))
}
if ($derivedId.ToString() -cne $extensionId) {
    throw 'O ID derivado de manifest.key nao confere com extension-id.txt.'
}

if ([string]::IsNullOrWhiteSpace($Executable)) {
    $candidates = @(
        (Join-Path $projectRoot 'dist/ponteproj.exe'),
        (Join-Path $projectRoot 'target/release/ponteproj.exe'),
        (Join-Path $projectRoot 'target/debug/ponteproj.exe')
    )
    $Executable = $candidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
    if ([string]::IsNullOrWhiteSpace($Executable)) {
        throw 'ponteproj.exe nao encontrado. Compile ou forneca -Executable com o caminho do executavel.'
    }
}
if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) {
    throw "Executavel nao encontrado: $Executable"
}
$executableItem = Get-Item -LiteralPath $Executable
if ($executableItem.Extension -ine '.exe') {
    throw 'O host precisa apontar para um executavel .exe do Windows.'
}
$executablePath = $executableItem.FullName

# The copy published in the Chrome Web Store has its own ID; when known
# (extension-store-id.txt), it is authorized alongside the unpacked one.
$allowedOrigins = @("chrome-extension://$extensionId/")
$storeIdPath = Join-Path $projectRoot 'extension-store-id.txt'
if (Test-Path -LiteralPath $storeIdPath -PathType Leaf) {
    $storeId = (Get-Content -LiteralPath $storeIdPath -Raw).Trim()
    if ($storeId -cmatch '^[a-p]{32}$' -and $storeId -cne $extensionId) {
        $allowedOrigins += "chrome-extension://$storeId/"
    } elseif ($storeId -ne '' -and $storeId -cne $extensionId) {
        throw 'extension-store-id.txt deve conter apenas o ID de 32 letras (a-p) da extensao publicada na loja.'
    }
}

$manifestDirectory = [System.IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'PonteProj'))
$manifestPath = Join-Path $manifestDirectory "$hostName.json"
$manifest = [ordered]@{
    name = $hostName
    description = 'PonteProj - assistente documental SIGADOC'
    path = $executablePath
    type = 'stdio'
    allowed_origins = $allowedOrigins
}
$manifestJson = $manifest | ConvertTo-Json -Depth 4
$registryPaths = @(
    "Software\Google\Chrome\NativeMessagingHosts\$hostName",
    "Software\Microsoft\Edge\NativeMessagingHosts\$hostName"
)

Write-Host "Executavel: $executablePath"
Write-Host "Manifesto: $manifestPath"
Write-Host "Extensao autorizada: $extensionId"
if (-not $PSCmdlet.ShouldProcess("$manifestPath e as duas chaves HKCU do host $hostName", 'Instalar/atualizar a ponte para Chrome e Edge')) {
    return
}

[void][System.IO.Directory]::CreateDirectory($manifestDirectory)
$temporaryPath = Join-Path $manifestDirectory "$hostName.install-$([guid]::NewGuid().ToString('N')).tmp"
# Verify both move targets stay in this explicitly named application directory.
foreach ($writePath in @($temporaryPath, $manifestPath)) {
    $resolvedParent = [System.IO.Path]::GetDirectoryName([System.IO.Path]::GetFullPath($writePath))
    if (-not [string]::Equals($resolvedParent, $manifestDirectory, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Destino do manifesto fora da pasta local PonteProj.'
    }
}
try {
    [System.IO.File]::WriteAllText($temporaryPath, $manifestJson, [System.Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $temporaryPath -Destination $manifestPath -Force
} finally {
    if (Test-Path -LiteralPath $temporaryPath -PathType Leaf) {
        Remove-Item -LiteralPath $temporaryPath -Force
    }
}

foreach ($registryPath in $registryPaths) {
    $registryKey = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($registryPath)
    if ($null -eq $registryKey) {
        throw "Nao foi possivel abrir HKCU\$registryPath"
    }
    try {
        $registryKey.SetValue('', $manifestPath, [Microsoft.Win32.RegistryValueKind]::String)
        if ($registryKey.GetValue('') -cne $manifestPath) {
            throw "Falha ao conferir HKCU\$registryPath"
        }
    } finally {
        $registryKey.Dispose()
    }
}

Write-Host 'Ponte registrada para o usuario atual. Reabra o navegador e use Verificar conexao com o aplicativo no popup da extensao.'
[pscustomobject]@{
    HostName = $hostName
    ExtensionId = $extensionId
    Executable = $executablePath
    ManifestPath = $manifestPath
    Browsers = @('Chrome', 'Edge')
}
