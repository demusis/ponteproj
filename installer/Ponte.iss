; Instalador do Ponte para o usuário atual (sem administrador).
; Compile com scripts\build-installer.ps1, que define /DAppVersion e, quando
; existir, /DStoreExtensionId (ID da extensão publicada na Chrome Web Store).
;
; O que o instalador faz:
;   1. faz uma cópia de segurança do acervo (acervo.sqlite3) antes de tudo;
;   2. copia o programa para %LOCALAPPDATA%\Programs\PonteProj\vX.Y.Z;
;   3. copia a extensão do navegador para ...\PonteProj\extension (pasta fixa);
;   4. grava o manifesto do host nativo e as chaves HKCU para Chrome e Edge;
;   5. cria o atalho no menu Iniciar e remove versões anteriores.
; Os dados (acervo, backups, exportados) nunca são apagados, nem na desinstalação.

#ifndef AppVersion
  #error Defina /DAppVersion=X.Y.Z (use scripts\build-installer.ps1)
#endif
#ifndef StoreExtensionId
  #define StoreExtensionId ""
#endif
#define AppName "Ponte"
#define HostName "com.ponteproj.assistente"
#define DevExtensionId "ecllnakmppjinmednoikimbpnbgbimhg"
#define Root ".."

[Setup]
AppId={{7C3C1E7A-6D7B-4B40-9C32-5A2F1D1B0A91}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName=Ponte {#AppVersion}
AppPublisher=Ponte — Assistente documental SIGADOC
DefaultDirName={localappdata}\Programs\PonteProj\v{#AppVersion}
UsePreviousAppDir=no
DisableDirPage=yes
DisableProgramGroupPage=yes
DefaultGroupName=Ponte
PrivilegesRequired=lowest
OutputDir={#Root}\dist
OutputBaseFilename=Ponte-Setup-{#AppVersion}
SetupIconFile={#Root}\assets\ponteproj.ico
UninstallDisplayIcon={app}\ponteproj.exe
UninstallDisplayName=Ponte
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes
CloseApplicationsFilter=*.exe
RestartApplications=no
InfoAfterFile=depois-da-instalacao.txt
ShowLanguageDialog=no
MinVersion=10.0
VersionInfoVersion={#AppVersion}
VersionInfoProductName=Ponte
VersionInfoDescription=Instalador do Ponte

[Languages]
Name: "br"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Messages]
br.WelcomeLabel2=Este assistente instala o [name/ver] só para a sua conta do Windows, sem precisar de administrador.%n%nFeche o Ponte, se estiver aberto, antes de continuar.

[Tasks]
Name: "desktopicon"; Description: "Criar atalho na área de trabalho"; Flags: unchecked

[Files]
Source: "{#Root}\dist\ponteproj.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\dist\*.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#Root}\extension\manifest.json"; DestDir: "{localappdata}\Programs\PonteProj\extension"; Flags: ignoreversion
Source: "{#Root}\extension\capture-core.js"; DestDir: "{localappdata}\Programs\PonteProj\extension"; Flags: ignoreversion
Source: "{#Root}\extension\files.js"; DestDir: "{localappdata}\Programs\PonteProj\extension"; Flags: ignoreversion
Source: "{#Root}\extension\history.js"; DestDir: "{localappdata}\Programs\PonteProj\extension"; Flags: ignoreversion
Source: "{#Root}\extension\content.js"; DestDir: "{localappdata}\Programs\PonteProj\extension"; Flags: ignoreversion
Source: "{#Root}\extension\background.js"; DestDir: "{localappdata}\Programs\PonteProj\extension"; Flags: ignoreversion
Source: "{#Root}\extension\popup.html"; DestDir: "{localappdata}\Programs\PonteProj\extension"; Flags: ignoreversion
Source: "{#Root}\extension\popup.js"; DestDir: "{localappdata}\Programs\PonteProj\extension"; Flags: ignoreversion
Source: "{#Root}\extension\popup.css"; DestDir: "{localappdata}\Programs\PonteProj\extension"; Flags: ignoreversion
Source: "{#Root}\extension\icons\icon-16.png"; DestDir: "{localappdata}\Programs\PonteProj\extension\icons"; Flags: ignoreversion
Source: "{#Root}\extension\icons\icon-32.png"; DestDir: "{localappdata}\Programs\PonteProj\extension\icons"; Flags: ignoreversion
Source: "{#Root}\extension\icons\icon-48.png"; DestDir: "{localappdata}\Programs\PonteProj\extension\icons"; Flags: ignoreversion
Source: "{#Root}\extension\icons\icon-128.png"; DestDir: "{localappdata}\Programs\PonteProj\extension\icons"; Flags: ignoreversion
Source: "{#Root}\GUIA_DO_USUARIO.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\Ponte"; Filename: "{app}\ponteproj.exe"; WorkingDir: "{app}"; Comment: "Ponte — assistente documental SIGADOC"
Name: "{userdesktop}\Ponte"; Filename: "{app}\ponteproj.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Google\Chrome\NativeMessagingHosts\{#HostName}"; ValueType: string; ValueName: ""; ValueData: "{localappdata}\PonteProj\{#HostName}.json"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Microsoft\Edge\NativeMessagingHosts\{#HostName}"; ValueType: string; ValueName: ""; ValueData: "{localappdata}\PonteProj\{#HostName}.json"; Flags: uninsdeletekey

[UninstallDelete]
Type: files; Name: "{localappdata}\PonteProj\{#HostName}.json"
Type: dirifempty; Name: "{localappdata}\Programs\PonteProj\extension\icons"
Type: dirifempty; Name: "{localappdata}\Programs\PonteProj\extension"
Type: dirifempty; Name: "{localappdata}\Programs\PonteProj"

[Run]
Filename: "{app}\ponteproj.exe"; Description: "Abrir o Ponte agora"; Flags: postinstall nowait skipifsilent

[Code]
var
  BackupPath: String;

function JsonEscape(const Value: String): String;
begin
  Result := Value;
  StringChangeEx(Result, '\', '\\', True);
  StringChangeEx(Result, '"', '\"', True);
end;

{ Cópia de segurança do acervo antes de qualquer alteração. Só roda com o
  Ponte fechado (CloseApplications já pediu para fechar). }
procedure BackupLibrary;
var
  DataDir, Source, Stamp: String;
begin
  BackupPath := '';
  DataDir := ExpandConstant('{localappdata}\PonteProj');
  Source := DataDir + '\acervo.sqlite3';
  if not FileExists(Source) then
    exit;
  Stamp := GetDateTimeString('yyyymmdd-hhnnss', '-', '-');
  ForceDirectories(DataDir + '\backups');
  BackupPath := DataDir + '\backups\antes-de-instalar-v{#AppVersion}-' + Stamp + '.sqlite3';
  if CopyFile(Source, BackupPath, False) then
  begin
    Log('Cópia de segurança do acervo: ' + BackupPath);
    if FileExists(Source + '-wal') then
      CopyFile(Source + '-wal', BackupPath + '-wal', False);
    if FileExists(Source + '-shm') then
      CopyFile(Source + '-shm', BackupPath + '-shm', False);
  end
  else
  begin
    Log('Falha na cópia de segurança do acervo.');
    BackupPath := '';
  end;
end;

{ Manifesto do host nativo: aponta o navegador para o executável instalado. }
procedure WriteHostManifest;
var
  ManifestPath, Exe, Origins: String;
  Lines: TArrayOfString;
begin
  Exe := JsonEscape(ExpandConstant('{app}\ponteproj.exe'));
  Origins := '"chrome-extension://{#DevExtensionId}/"';
#if StoreExtensionId != ""
  Origins := Origins + ', "chrome-extension://{#StoreExtensionId}/"';
#endif
  SetArrayLength(Lines, 7);
  Lines[0] := '{';
  Lines[1] := '  "name": "{#HostName}",';
  Lines[2] := '  "description": "Ponte - assistente documental SIGADOC",';
  Lines[3] := '  "path": "' + Exe + '",';
  Lines[4] := '  "type": "stdio",';
  Lines[5] := '  "allowed_origins": [' + Origins + ']';
  Lines[6] := '}';
  ManifestPath := ExpandConstant('{localappdata}\PonteProj\{#HostName}.json');
  ForceDirectories(ExtractFileDir(ManifestPath));
  if not SaveStringsToUTF8FileWithoutBOM(ManifestPath, Lines, False) then
    MsgBox('Não foi possível gravar a ligação com o navegador em ' + ManifestPath + '. Abra o Ponte e confira a tela inicial.', mbError, MB_OK);
end;

{ Versões anteriores e restos do instalador antigo por script. }
procedure RemoveOldVersions;
var
  Base, Current: String;
  FindRec: TFindRec;
begin
  Base := ExpandConstant('{localappdata}\Programs\PonteProj');
  Current := 'v{#AppVersion}';
  if FindFirst(Base + '\v*', FindRec) then
  begin
    try
      repeat
        if ((FindRec.Attributes and FILE_ATTRIBUTE_DIRECTORY) <> 0)
          and (Length(FindRec.Name) > 1)
          and (FindRec.Name[1] = 'v')
          and (FindRec.Name[2] >= '0') and (FindRec.Name[2] <= '9')
          and (CompareText(FindRec.Name, Current) <> 0) then
        begin
          if DelTree(Base + '\' + FindRec.Name, True, True, True) then
            Log('Versão anterior removida: ' + FindRec.Name)
          else
            Log('Versão anterior mantida (em uso): ' + FindRec.Name);
        end;
      until not FindNext(FindRec);
    finally
      FindClose(FindRec);
    end;
  end;
  DeleteFile(Base + '\ponteproj.exe');
  DeleteFile(Base + '\extension-id.txt');
  DeleteFile(Base + '\extension-key.txt');
  DeleteFile(ExpandConstant('{userprograms}\PonteProj.lnk'));
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then
    BackupLibrary;
  if CurStep = ssPostInstall then
  begin
    WriteHostManifest;
    RemoveOldVersions;
  end;
end;
