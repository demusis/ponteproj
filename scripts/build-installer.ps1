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
    [string]$InnoCompiler,
    # Pula so a conferencia de data (nao a de versao): use apenas se souber que
    # os fontes mudaram depois do build sem alterar o programa.
    [switch]$IgnoreFreshness
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

# O instalador nao pode levar um executavel velho: a versao dele tem de ser a de
# Cargo.toml e ele nao pode ser mais antigo que os fontes.
function Get-ExecutableVersion([string]$Path) {
    # O executavel nao traz recurso de versao do Windows; ele responde a --version
    # ("PonteProj X.Y.Z"). Se nao responder (versao antiga demais), devolve $null.
    $out = Join-Path ([System.IO.Path]::GetTempPath()) ("ponte-version-" + [guid]::NewGuid().ToString('N') + '.txt')
    try {
        $process = Start-Process -FilePath $Path -ArgumentList '--version' -RedirectStandardOutput $out -WindowStyle Hidden -PassThru
        if (-not $process.WaitForExit(15000)) {
            try { $process.Kill() } catch { }
            return $null
        }
        if (Test-Path -LiteralPath $out) {
            $text = Get-Content -LiteralPath $out -Raw -ErrorAction SilentlyContinue
            if ($text -and $text -match 'PonteProj\s+([0-9]+\.[0-9]+\.[0-9]+)') { return $Matches[1] }
        }
        return $null
    } catch {
        return $null
    } finally {
        Remove-Item -LiteralPath $out -Force -ErrorAction SilentlyContinue
    }
}

$exeVersion = Get-ExecutableVersion $executable
if ($exeVersion -and $exeVersion -ne $version) {
    throw "dist\ponteproj.exe e da versao $exeVersion, mas Cargo.toml esta na $version. Gere o release de novo (scripts\build.ps1 -Action Release) e copie o executavel novo para dist."
}
if (-not $exeVersion) {
    Write-Warning 'Nao foi possivel ler a versao de dist\ponteproj.exe (--version); conferindo so a data de gravacao.'
}
if (-not $IgnoreFreshness) {
    # Exactly what the executable is built from: the crate (Cargo.toml,
    # Cargo.lock, build.rs, every file under src), the files the sources
    # embed from outside src (grep include_str!/include_bytes! outside tests:
    # GUIA_DO_USUARIO.md, extension-id.txt, extension-store-id.txt and the
    # Phosphor font) and what build.rs turns into the Windows icon resource.
    # Anything else in the repository (the other assets, extension\, mobile\,
    # docs) does not change ponteproj.exe.
    $sources = @()
    $srcFolder = Join-Path $projectRoot 'src'
    if (Test-Path -LiteralPath $srcFolder -PathType Container) {
        $sources += Get-ChildItem -LiteralPath $srcFolder -Recurse -File
    }
    foreach ($relative in @(
            'Cargo.toml', 'Cargo.lock', 'build.rs',
            'GUIA_DO_USUARIO.md', 'extension-id.txt', 'extension-store-id.txt',
            'assets\fonts\Phosphor.ttf', 'assets\app.rc', 'assets\ponteproj.ico')) {
        $file = Join-Path $projectRoot $relative
        if (Test-Path -LiteralPath $file -PathType Leaf) { $sources += Get-Item -LiteralPath $file }
    }
    $newest = $sources | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
    $built = (Get-Item -LiteralPath $executable).LastWriteTimeUtc
    if ($newest.LastWriteTimeUtc -gt $built) {
        $newestName = $newest.FullName.Substring($projectRoot.Length + 1)
        throw ("dist\ponteproj.exe ({0:yyyy-MM-dd HH:mm:ss} UTC) e mais antigo que {1} ({2:yyyy-MM-dd HH:mm:ss} UTC). Gere o release de novo (scripts\build.ps1 -Action Release) e copie o executavel novo para dist. Um 'git checkout' ou 'git pull' recente tambem muda a data dos arquivos sem mudar o programa: se for o caso, recompile mesmo assim (o cargo so refaz o que mudou), copie o executavel novo para dist e rode este script de novo com -IgnoreFreshness." -f $built, $newestName, $newest.LastWriteTimeUtc)
    }
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
