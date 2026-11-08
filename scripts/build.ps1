#Requires -Version 5.1
<#
.SYNOPSIS
Verifica, testa ou compila o PonteProj no Windows.
.PARAMETER Action
Check, Test, Release ou All. O padrao Release produz target/release/ponteproj.exe.
Lint roda o clippy em todo o codigo (avisos viram erro); Format confere a
formatacao com rustfmt sem alterar arquivos. LintFix e FormatFix aplicam as
correcoes automaticas do clippy e do rustfmt (alteram arquivos).
.PARAMETER ToolsDirectory
Pasta opcional de ferramentas portateis contendo cargo, rustup e o conjunto
MinGW completo em mingw/w64devkit/bin. Sem ela, usa Cargo/Rust do PATH.
.EXAMPLE
.\scripts\build.ps1 -Action All -ToolsDirectory C:\Ferramentas\ponte-tools
.EXAMPLE
.\scripts\build.ps1 -Action Check -WhatIf
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [ValidateSet('Check', 'Test', 'Release', 'All', 'Lint', 'LintFix', 'Format', 'FormatFix', 'AiSmoke')]
    [string]$Action = 'Release',
    [string]$ToolsDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') {
    throw 'Esta implementacao e seus testes de protecao requerem Windows.'
}

$projectRoot = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path -LiteralPath (Join-Path $projectRoot 'Cargo.toml') -PathType Leaf)) {
    throw 'Cargo.toml nao encontrado na raiz do projeto.'
}

$environmentChanges = @{}
if (-not [string]::IsNullOrWhiteSpace($ToolsDirectory)) {
    if (-not (Test-Path -LiteralPath $ToolsDirectory -PathType Container)) {
        throw "Pasta de ferramentas nao encontrada: $ToolsDirectory"
    }
    $toolsRoot = (Get-Item -LiteralPath $ToolsDirectory).FullName
    $portableCargoHome = Join-Path $toolsRoot 'cargo'
    $portableRustupHome = Join-Path $toolsRoot 'rustup'
    $cargoBin = Join-Path $portableCargoHome 'bin'
    $toolchainName = 'stable-x86_64-pc-windows-gnu'
    $toolchainRoot = Join-Path $portableRustupHome "toolchains/$toolchainName"
    $toolchainBin = Join-Path $toolchainRoot 'bin'
    $rustlibBin = Join-Path $toolchainRoot 'lib/rustlib/x86_64-pc-windows-gnu/bin'
    $selfContainedBin = Join-Path $rustlibBin 'self-contained'
    $mingwBin = Join-Path $toolsRoot 'mingw/w64devkit/bin'
    $cargoExecutable = Join-Path $cargoBin 'cargo.exe'
    $linkerExecutable = Join-Path $selfContainedBin 'x86_64-w64-mingw32-gcc.exe'
    $ccExecutable = Join-Path $mingwBin 'gcc.exe'
    $arExecutable = Join-Path $mingwBin 'ar.exe'
    $dlltoolExecutable = Join-Path $mingwBin 'dlltool.exe'
    foreach ($requiredFile in @($cargoExecutable, (Join-Path $cargoBin 'rustc.exe'), $linkerExecutable, $ccExecutable, $arExecutable, $dlltoolExecutable)) {
        if (-not (Test-Path -LiteralPath $requiredFile -PathType Leaf)) {
            throw "Ferramenta portatil ausente: $requiredFile"
        }
    }
    foreach ($requiredDirectory in @($portableRustupHome, $toolchainBin, $rustlibBin, $selfContainedBin, $mingwBin)) {
        if (-not (Test-Path -LiteralPath $requiredDirectory -PathType Container)) {
            throw "Pasta do toolchain ausente: $requiredDirectory"
        }
    }

    # Encoded arguments keep a dlltool path containing spaces as one argument.
    # Preserve existing flags using the same precedence and whitespace rules as Cargo.
    $inheritedEncodedFlags = [System.Environment]::GetEnvironmentVariable('CARGO_ENCODED_RUSTFLAGS', 'Process')
    $inheritedPlainFlags = [System.Environment]::GetEnvironmentVariable('RUSTFLAGS', 'Process')
    $rustFlagsToUse = @()
    if (-not [string]::IsNullOrEmpty($inheritedEncodedFlags)) {
        $rustFlagsToUse = @($inheritedEncodedFlags -split [char]31)
    } elseif (-not [string]::IsNullOrWhiteSpace($inheritedPlainFlags)) {
        $rustFlagsToUse = @($inheritedPlainFlags.Trim() -split '\s+')
    }
    $rustFlagsToUse += @('-C', "dlltool=$dlltoolExecutable", '-C', 'link-self-contained=yes')

    $environmentChanges = @{
        'CARGO_HOME' = $portableCargoHome
        'RUSTUP_HOME' = $portableRustupHome
        'RUSTUP_TOOLCHAIN' = $toolchainName
        'PATH' = (@($mingwBin, $cargoBin, $toolchainBin, $rustlibBin, $selfContainedBin, $env:PATH) -join ';')
        'CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER' = $linkerExecutable
        'CC' = $ccExecutable
        'AR' = $arExecutable
        'CC_x86_64_pc_windows_gnu' = $ccExecutable
        'AR_x86_64_pc_windows_gnu' = $arExecutable
        'CARGO_ENCODED_RUSTFLAGS' = ($rustFlagsToUse -join [char]31)
        # Keep the portable toolchain's native target and ordinary target/ outputs.
        'CARGO_BUILD_TARGET' = $null
    }
    Write-Host "Ferramentas portateis: $toolsRoot"
    Write-Host "Linker GNU fornecido pelo Rust: $linkerExecutable"
    Write-Host "Compilador C MinGW: $ccExecutable"
    Write-Host "Arquivador MinGW: $arExecutable"
    Write-Host "Gerador de import libraries MinGW: $dlltoolExecutable"
} else {
    $cargoCommand = Get-Command cargo -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -eq $cargoCommand) {
        throw 'Cargo nao encontrado no PATH. Instale Rust ou forneca -ToolsDirectory com o conjunto portatil.'
    }
    $cargoExecutable = $cargoCommand.Source
    Write-Host "Cargo do PATH: $cargoExecutable"
}

$actions = if ($Action -eq 'All') { @('Check', 'Test', 'Release') } else { @($Action) }
$cargoArguments = @{
    'Check' = @('check', '--all-targets', '--locked')
    'Test' = @('test', '--all-targets', '--locked')
    'Release' = @('build', '--release', '--locked')
    'Lint' = @('clippy', '--all-targets', '--locked', '--', '-D', 'warnings')
    'Format' = @('fmt', '--all', '--check')
    'LintFix' = @('clippy', '--fix', '--allow-dirty', '--allow-staged', '--all-targets', '--locked')
    'FormatFix' = @('fmt', '--all')
    'AiSmoke' = @('test', '--lib', '--locked', 'live_context_smoke', '--', '--ignored', '--nocapture')
}
foreach ($step in $actions) {
    Write-Host ("Planejado: cargo " + ($cargoArguments[$step] -join ' '))
}
if (-not $PSCmdlet.ShouldProcess($projectRoot, "Executar Cargo: $($actions -join ', ')")) {
    return
}

$previousEnvironment = @{}
$locationChanged = $false
try {
    foreach ($name in $environmentChanges.Keys) {
        $previousEnvironment[$name] = [System.Environment]::GetEnvironmentVariable($name, 'Process')
        if ($null -eq $environmentChanges[$name]) {
            Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue
        } else {
            [System.Environment]::SetEnvironmentVariable($name, $environmentChanges[$name], 'Process')
        }
    }
    Push-Location -LiteralPath $projectRoot
    $locationChanged = $true
    foreach ($step in $actions) {
        $stepArguments = $cargoArguments[$step]
        & $cargoExecutable @stepArguments
        if ($LASTEXITCODE -ne 0) {
            throw "Cargo $step terminou com codigo $LASTEXITCODE. As etapas seguintes nao foram executadas."
        }
    }
    if ($actions -contains 'Release') {
        Write-Host "Executavel: $(Join-Path $projectRoot 'target/release/ponteproj.exe')"
    }
} finally {
    if ($locationChanged) {
        Pop-Location
    }
    foreach ($name in $previousEnvironment.Keys) {
        if ($null -eq $previousEnvironment[$name]) {
            Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue
        } else {
            [System.Environment]::SetEnvironmentVariable($name, $previousEnvironment[$name], 'Process')
        }
    }
}
