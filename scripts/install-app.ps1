#Requires -Version 5.1
<#
.SYNOPSIS
Instala os arquivos de runtime PonteProj para o usuario atual e registra o host.
.DESCRIPTION
Copia dist/ponteproj.exe, dist/*.dll, os arquivos de runtime da extensao e sua
identidade fixa para a pasta pessoal real LOCALAPPDATA/Programs/PonteProj.
Cria PonteProj.lnk no menu Iniciar e chama o install-native-host.ps1 original.
Nao remove arquivos preexistentes nem instala testes/dependencias de desenvolvimento.
Antes de copiar qualquer arquivo, pede ao ponteproj.exe que vai ser instalado uma
copia de seguranca consistente do acervo (ponteproj.exe --copia-antes-de-atualizar):
ele acha o acervo como o Ponte acha (inclusive em PONTEPROJ_DATA_DIR), usa o backup
online do SQLite (o conteudo ainda no -wal entra na copia), confere a copia e a grava
em <pasta do acervo>\backups. Se a copia falhar, a instalacao para; -SemCopia ignora
essa protecao (nao recomendado).
Use -WhatIf para conferir o plano completo sem criar pastas, copiar arquivos,
gravar o atalho, alterar o ambiente ou registrar o host.
#>
[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'Medium')]
param(
    [ValidatePattern('^v[0-9]+\.[0-9]+\.[0-9]+$')]
    [string]$VersionSubdirectory,
    # Instala mesmo sem a copia de seguranca do acervo. Nao recomendado.
    [switch]$SemCopia
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

# Safety copy of the library: made by the executable being installed, which finds
# the library the way Ponte does (PONTEPROJ_DATA_DIR included) and takes a
# consistent SQLite backup (the WAL is part of it), so it works with Ponte open
# and does not depend on a fixed folder. The name is kept: backups\antes-de-instalar-<versao>-<data>.sqlite3.
$backupLabel = if ($VersionSubdirectory) { $VersionSubdirectory } else { 'sem-versao' }

Write-Host "Pasta do aplicativo: $installDirectory"
Write-Host "Extensao para carregar no navegador: $installedExtension"
Write-Host "Atalho no menu Iniciar: $shortcutPath"
if ($SemCopia) {
    Write-Host 'Copia de seguranca do acervo: DESLIGADA (-SemCopia)'
} else {
    Write-Host 'Copia de seguranca do acervo: feita pelo ponteproj.exe novo, na pasta backups do acervo'
}

$operation = 'Copiar/atualizar somente o runtime, criar PonteProj.lnk e registrar o host nativo para este usuario'
if (-not $PSCmdlet.ShouldProcess("$installDirectory; $shortcutPath; host com.ponteproj.assistente em HKCU", $operation)) {
    return
}

$backupPath = $null
if ($SemCopia) {
    Write-Warning 'Instalando SEM copia de seguranca do acervo (-SemCopia).'
} else {
    $reportFile = Join-Path ([System.IO.Path]::GetTempPath()) ('ponte-copia-' + [guid]::NewGuid().ToString('N') + '.txt')
    try {
        # Start-Process -Wait: ponteproj.exe is a Windows GUI program, which the
        # call operator does not wait for.
        $backupProcess = Start-Process -FilePath $sourceExecutable -ArgumentList @('--copia-antes-de-atualizar', '--rotulo', $backupLabel, '--resultado', ('"' + $reportFile + '"')) -WindowStyle Hidden -Wait -PassThru
        # @(if ...) keeps an array even when the report is absent (nothing) or has a single line
        # (a bare string): .Count and [1..] need an array under Set-StrictMode on Windows PowerShell 5.1.
        $report = @(if (Test-Path -LiteralPath $reportFile -PathType Leaf) { Get-Content -LiteralPath $reportFile -Encoding UTF8 })
        $status = if ($report.Count -gt 0) { $report[0].Trim() } else { '' }
        $detail = if ($report.Count -gt 1) { ($report[1..($report.Count - 1)] -join ' ').Trim() } else { '' }
        if ($backupProcess.ExitCode -ne 0) {
            if (-not $detail) { $detail = "O programa de copia terminou com o codigo $($backupProcess.ExitCode)." }
            throw "Nao foi possivel fazer a copia de seguranca do acervo; nada foi instalado. $detail Confira o espaco em disco e a permissao de gravacao na pasta backups do acervo (LOCALAPPDATA\PonteProj\backups, ou a pasta definida em PONTEPROJ_DATA_DIR). Para instalar mesmo assim, use -SemCopia (nao recomendado)."
        }
        if ($status -eq 'ok' -and $detail) { $backupPath = $detail }
    } finally {
        Remove-Item -LiteralPath $reportFile -Force -ErrorAction SilentlyContinue
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
