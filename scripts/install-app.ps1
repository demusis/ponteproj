#Requires -Version 5.1
<#
.SYNOPSIS
Instala os arquivos de runtime PonteProj para o usuario atual e registra o host.
.DESCRIPTION
Copia dist/ponteproj.exe, dist/*.dll, os arquivos de runtime da extensao e sua
identidade fixa para a pasta pessoal real LOCALAPPDATA/Programs/PonteProj.
Cria PonteProj.lnk no menu Iniciar e chama o install-native-host.ps1 original.
Nao remove arquivos preexistentes nem instala testes/dependencias de desenvolvimento.
Use -WhatIf para conferir o plano completo sem criar pastas, copiar arquivos,
gravar o atalho, alterar o ambiente ou registrar o host.
#>
[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'Medium')]
param(
    [ValidatePattern('^v[0-9]+\.[0-9]+\.[0-9]+$')]
    [string]$VersionSubdirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($env:OS -ne 'Windows_NT') {
    throw 'A instalacao PonteProj requer Windows.'
}

$projectRoot = Split-Path -Parent $PSScriptRoot
$sourceExecutable = Join-Path $projectRoot 'dist/ponteproj.exe'
$nativeInstaller = Join-Path $PSScriptRoot 'install-native-host.ps1'
$extensionIdPath = Join-Path $projectRoot 'extension-id.txt'
$extensionKeyPath = Join-Path $projectRoot 'extension-key.txt'
$extensionManifestPath = Join-Path $projectRoot 'extension/manifest.json'
$expectedExtensionId = 'ecllnakmppjinmednoikimbpnbgbimhg'

# This is the runtime of the current package, not the entire source directory.
# In particular, package.json, README, tests, node_modules and toolchains stay out.
$extensionRuntimeFiles = @(
    'manifest.json',
    'capture-core.js',
    'files.js',
    'history.js',
    'content.js',
    'background.js',
    'whatsapp-core.js',
    'whatsapp.js',
    'popup.html',
    'popup.js',
    'popup.css',
    'icons/icon-16.png',
    'icons/icon-32.png',
    'icons/icon-48.png',
    'icons/icon-128.png'
)
$requiredPaths = @($sourceExecutable, $nativeInstaller, $extensionIdPath, $extensionKeyPath)
foreach ($relativePath in $extensionRuntimeFiles) {
    $requiredPaths += Join-Path (Join-Path $projectRoot 'extension') $relativePath
}
foreach ($requiredPath in $requiredPaths) {
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
        throw "Arquivo de runtime do pacote nao encontrado: $requiredPath"
    }
}

# Validate the identity before any files are written. The original host installer
# repeats this check when registering the installed executable.
$extensionId = (Get-Content -LiteralPath $extensionIdPath -Raw).Trim()
$publicKey = (Get-Content -LiteralPath $extensionKeyPath -Raw).Trim()
$extensionManifest = Get-Content -LiteralPath $extensionManifestPath -Raw | ConvertFrom-Json
if ($extensionId -cne $expectedExtensionId -or $extensionManifest.key -cne $publicKey) {
    throw 'A identidade fixa da extensao diverge do pacote. Preserve extension-id.txt, extension-key.txt e manifest.key.'
}
$hasher = [System.Security.Cryptography.SHA256]::Create()
try {
    $keyHash = $hasher.ComputeHash([Convert]::FromBase64String($publicKey))
} finally {
    $hasher.Dispose()
}
$derivedId = [System.Text.StringBuilder]::new(32)
for ($index = 0; $index -lt 16; $index++) {
    [void]$derivedId.Append([char](97 + ($keyHash[$index] -shr 4)))
    [void]$derivedId.Append([char](97 + ($keyHash[$index] -band 15)))
}
if ($derivedId.ToString() -cne $expectedExtensionId) {
    throw 'A chave publica nao produz o ID fixo esperado da extensao.'
}

# Use the Windows personal folder API. Do not try to escape sandbox redirection;
# the installer must run from a normal user terminal when that is present.
$localAppData = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
if ([string]::IsNullOrWhiteSpace($localAppData)) { $localAppData = $env:LOCALAPPDATA }
if ([string]::IsNullOrWhiteSpace($localAppData) -or -not [System.IO.Path]::IsPathRooted($localAppData) -or $localAppData -match '(?i)#!sandbox\.') {
    throw 'LOCALAPPDATA esta indisponivel ou redirecionado para um sandbox. Execute este script em um terminal normal do Windows, como o usuario atual.'
}
$localAppData = [System.IO.Path]::GetFullPath($localAppData)
$installDirectory = [System.IO.Path]::GetFullPath((Join-Path $localAppData 'Programs/PonteProj'))
$binaryDirectory = if ($VersionSubdirectory) { Join-Path $installDirectory $VersionSubdirectory } else { $installDirectory }
$installedExecutable = Join-Path $binaryDirectory 'ponteproj.exe'
$installedExtension = Join-Path $installDirectory 'extension'
$startMenuDirectory = [Environment]::GetFolderPath([Environment+SpecialFolder]::Programs)
if ([string]::IsNullOrWhiteSpace($startMenuDirectory) -or -not [System.IO.Path]::IsPathRooted($startMenuDirectory)) {
    throw 'Nao foi possivel localizar o menu Iniciar pessoal do usuario atual.'
}
$shortcutPath = Join-Path $startMenuDirectory 'PonteProj.lnk'

$copyPlan = @(
    [pscustomobject]@{ Source = $sourceExecutable; Destination = $installedExecutable },
    [pscustomobject]@{ Source = $extensionIdPath; Destination = (Join-Path $installDirectory 'extension-id.txt') },
    [pscustomobject]@{ Source = $extensionKeyPath; Destination = (Join-Path $installDirectory 'extension-key.txt') }
)
foreach ($dll in @(Get-ChildItem -LiteralPath (Join-Path $projectRoot 'dist') -Filter '*.dll' -File)) {
    $copyPlan += [pscustomobject]@{ Source = $dll.FullName; Destination = (Join-Path $binaryDirectory $dll.Name) }
}
foreach ($relativePath in $extensionRuntimeFiles) {
    $copyPlan += [pscustomobject]@{
        Source = (Join-Path (Join-Path $projectRoot 'extension') $relativePath)
        Destination = (Join-Path $installedExtension $relativePath)
    }
}

# Refuse destinations that escape the explicitly named application directory.
$installPrefix = $installDirectory.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
foreach ($entry in $copyPlan) {
    $resolvedDestination = [System.IO.Path]::GetFullPath($entry.Destination)
    if (-not $resolvedDestination.StartsWith($installPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Destino fora da pasta PonteProj: $resolvedDestination"
    }
}
foreach ($directory in @($installDirectory, $installedExtension, (Join-Path $installedExtension 'icons'), $binaryDirectory)) {
    if (Test-Path -LiteralPath $directory) {
        $directoryItem = Get-Item -LiteralPath $directory -Force
        if (-not $directoryItem.PSIsContainer -or ($directoryItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
            throw "O destino precisa ser uma pasta comum, sem redirecionamento: $directory"
        }
    }
}
foreach ($entry in $copyPlan) {
    if (Test-Path -LiteralPath $entry.Destination) {
        $destinationItem = Get-Item -LiteralPath $entry.Destination -Force
        if ($destinationItem.PSIsContainer -or ($destinationItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
            throw "Destino de arquivo invalido ou redirecionado: $($entry.Destination)"
        }
    }
}

# A running Ponte keeps its executable and database open; installing over it
# would fail half-way. Ask the person to close it instead.
$runningPonte = @(Get-Process -Name 'ponteproj' -ErrorAction SilentlyContinue)
if ($runningPonte.Count -gt 0) {
    throw 'O Ponte esta aberto. Feche a janela do Ponte (e espere a janela da extensao terminar) antes de instalar.'
}

# Safety copy of the library before anything changes. The database is only
# copied while no Ponte is running, so the copy is consistent.
$dataDirectory = Join-Path $localAppData 'PonteProj'
$databasePath = Join-Path $dataDirectory 'acervo.sqlite3'
$backupDirectory = Join-Path $dataDirectory 'backups'
$backupPath = $null
if (Test-Path -LiteralPath $databasePath -PathType Leaf) {
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $version = if ($VersionSubdirectory) { $VersionSubdirectory } else { 'sem-versao' }
    $backupPath = Join-Path $backupDirectory "antes-de-instalar-$version-$stamp.sqlite3"
}

Write-Host "Pasta do aplicativo: $installDirectory"
Write-Host "Extensao para carregar no navegador: $installedExtension"
Write-Host "Atalho no menu Iniciar: $shortcutPath"
if ($backupPath) { Write-Host "Copia de seguranca do acervo: $backupPath" }

$operation = 'Copiar/atualizar somente o runtime, criar PonteProj.lnk e registrar o host nativo para este usuario'
if (-not $PSCmdlet.ShouldProcess("$installDirectory; $shortcutPath; host com.ponteproj.assistente em HKCU", $operation)) {
    return
}

if ($backupPath) {
    [void][System.IO.Directory]::CreateDirectory($backupDirectory)
    Copy-Item -LiteralPath $databasePath -Destination $backupPath -Force
    foreach ($suffix in @('-wal', '-shm')) {
        $sidecar = "$databasePath$suffix"
        if (Test-Path -LiteralPath $sidecar -PathType Leaf) {
            Copy-Item -LiteralPath $sidecar -Destination "$backupPath$suffix" -Force
        }
    }
}

# Every mutation is inside the single approved installation operation above.
# Copy only named package files; never delete or mirror an existing directory.
[void][System.IO.Directory]::CreateDirectory($installDirectory)
[void][System.IO.Directory]::CreateDirectory($binaryDirectory)
[void][System.IO.Directory]::CreateDirectory($installedExtension)
[void][System.IO.Directory]::CreateDirectory((Join-Path $installedExtension 'icons'))
foreach ($entry in $copyPlan) {
    Copy-Item -LiteralPath $entry.Source -Destination $entry.Destination -Force
}

# Keep the original host installer's manifest and registry logic. Its environment
# is scoped to this call and restored even if registration fails.
$previousLocalAppData = $env:LOCALAPPDATA
try {
    $env:LOCALAPPDATA = $localAppData
    & $nativeInstaller -Executable $installedExecutable -Confirm:$false
} finally {
    $env:LOCALAPPDATA = $previousLocalAppData
}

[void][System.IO.Directory]::CreateDirectory($startMenuDirectory)
$shortcutShell = $null
$shortcut = $null
try {
    $shortcutShell = New-Object -ComObject WScript.Shell
    $shortcut = $shortcutShell.CreateShortcut($shortcutPath)
    $shortcut.TargetPath = $installedExecutable
    $shortcut.WorkingDirectory = $binaryDirectory
    $shortcut.Arguments = ''
    $shortcut.Description = 'PonteProj - assistente documental SIGADOC'
    $shortcut.IconLocation = "$installedExecutable,0"
    $shortcut.WindowStyle = 1
    $shortcut.Save()
} finally {
    if ($null -ne $shortcut) { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shortcut) }
    if ($null -ne $shortcutShell) { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shortcutShell) }
}

# Older versions stay only as long as nothing is pointing at them. The shortcut
# and the browser registration already point at the new one, so they are
# removed to avoid opening an old version by mistake.
$removedVersions = @()
if ($VersionSubdirectory) {
    foreach ($candidate in @(Get-ChildItem -LiteralPath $installDirectory -Directory -ErrorAction SilentlyContinue)) {
        if ($candidate.Name -match '^v[0-9]+\.[0-9]+\.[0-9]+$' -and $candidate.Name -ne $VersionSubdirectory) {
            try {
                Remove-Item -LiteralPath $candidate.FullName -Recurse -Force -ErrorAction Stop
                $removedVersions += $candidate.Name
            } catch {
                Write-Warning "Versao anterior mantida (em uso ou protegida): $($candidate.FullName)"
            }
        }
    }
}

Write-Host ''
Write-Host 'Ponte instalado.'
Write-Host "  Programa: $installedExecutable"
Write-Host "  Extensao do navegador (pasta para 'Carregar sem compactacao'): $installedExtension"
if ($backupPath) { Write-Host "  Copia de seguranca do acervo: $backupPath" }
if ($removedVersions.Count -gt 0) { Write-Host "  Versoes anteriores removidas: $($removedVersions -join ', ')" }
Write-Host 'Feche e abra o navegador. Depois abra o Ponte pelo menu Iniciar.'
[pscustomobject]@{
    InstallDirectory = $installDirectory
    Executable = $installedExecutable
    ExtensionDirectory = $installedExtension
    ExtensionId = $extensionId
    Shortcut = $shortcutPath
    RuntimeFilesCopied = $copyPlan.Count
    Backup = $backupPath
    RemovedVersions = $removedVersions
}
