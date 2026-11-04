@echo off
setlocal EnableExtensions
title Instalar o Ponte
echo.
echo  Ponte - Assistente documental SIGADOC
echo  Este instalador copia o programa e a extensao do navegador para a sua
echo  conta do Windows e liga o Chrome e o Edge ao Ponte. Nao precisa de
echo  administrador. Feche o Ponte antes de continuar.
echo.

set "VER="
for /f "tokens=2 delims== " %%v in ('findstr /b /c:"version = " "%~dp0Cargo.toml"') do if not defined VER set "VER=%%~v"
if not defined VER (
    echo  Nao foi possivel ler a versao em Cargo.toml.
    pause
    exit /b 1
)

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\install-app.ps1" -VersionSubdirectory "v%VER%" -Confirm:$false
set "RESULT=%ERRORLEVEL%"
echo.
if "%RESULT%"=="0" (
    echo  Pronto. Proximos passos:
    echo   1. Feche e abra o navegador.
    echo   2. Abra o Ponte pelo menu Iniciar: a tela inicial mostra se o navegador
    echo      esta ligado e tem o botao "Copiar caminho" da extensao.
    echo   3. No navegador, abra chrome://extensions (ou edge://extensions),
    echo      ative o "Modo do desenvolvedor", clique em "Carregar sem compactacao"
    echo      e cole o caminho copiado.
) else (
    echo  A instalacao nao foi concluida. Leia a mensagem acima.
)
echo.
pause
exit /b %RESULT%
