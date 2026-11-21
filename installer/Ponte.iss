; Instalador do Ponte para o usuário atual (sem administrador).
; Compile com scripts\build-installer.ps1, que define /DAppVersion e, quando
; existir, /DStoreExtensionId (ID da extensão publicada na Chrome Web Store).
;
; O que o instalador faz:
;   1. faz uma cópia de segurança consistente do acervo (acervo.sqlite3) antes de
;      tudo, com o próprio programa novo (ponteproj.exe --copia-antes-de-atualizar):
;      ele acha o acervo como o Ponte acha (inclusive em PONTEPROJ_DATA_DIR),
;      confere a cópia e grava em <pasta do acervo>\backups. Se a cópia falhar, a
;      instalação não segue (na instalação silenciosa, termina com código 8);
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
; O mesmo executável, só para a cópia de segurança antes de instalar (ExtractTemporaryFile).
Source: "{#Root}\dist\ponteproj.exe"; DestDir: "{tmp}"; Flags: dontcopy
Source: "{#Root}\extension\manifest.json"; DestDir: "{localappdata}\Programs\PonteProj\extension"; Flags: ignoreversion
Source: "{#Root}\extension\capture-core.js"; DestDir: "{localappdata}\Programs\PonteProj\extension"; Flags: ignoreversion
Source: "{#Root}\extension\files.js"; DestDir: "{localappdata}\Programs\PonteProj\extension"; Flags: ignoreversion
Source: "{#Root}\extension\history.js"; DestDir: "{localappdata}\Programs\PonteProj\extension"; Flags: ignoreversion
Source: "{#Root}\extension\content.js"; DestDir: "{localappdata}\Programs\PonteProj\extension"; Flags: ignoreversion
Source: "{#Root}\extension\background.js"; DestDir: "{localappdata}\Programs\PonteProj\extension"; Flags: ignoreversion
Source: "{#Root}\extension\whatsapp-core.js"; DestDir: "{localappdata}\Programs\PonteProj\extension"; Flags: ignoreversion
Source: "{#Root}\extension\whatsapp.js"; DestDir: "{localappdata}\Programs\PonteProj\extension"; Flags: ignoreversion
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

{ O relatório que ponteproj.exe --copia-antes-de-atualizar grava (UTF-8): a
  primeira linha é ok, sem-acervo ou erro; o resto é o detalhe. }
function ReadBackupReport(const ReportFile: String; var Detail: String): String;
var
  Raw: AnsiString;
  Text: String;
  Cut: Integer;
begin
  Result := '';
  Detail := '';
  if not LoadStringFromFile(ReportFile, Raw) then
    exit;
  Text := UTF8Decode(Raw);
  Cut := Pos(#10, Text);
  if Cut = 0 then
    Result := Trim(Text)
  else
  begin
    Result := Trim(Copy(Text, 1, Cut - 1));
    Detail := Trim(Copy(Text, Cut + 1, Length(Text)));
  end;
end;

{ Cópia de segurança consistente do acervo, feita pelo programa que vai ser
  instalado, antes de qualquer arquivo ser trocado. Devolve True quando há uma
  cópia conferida ou quando ainda não existe acervo. Detail diz o motivo da falha. }
function BackupLibrary(var Detail: String): Boolean;
var
  Exe, ReportFile, Params, Status, ReportDetail: String;
  ResultCode: Integer;
begin
  Result := False;
  Detail := '';
  BackupPath := '';
  Exe := ExpandConstant('{tmp}\ponteproj.exe');
  ReportFile := ExpandConstant('{tmp}\copia-do-acervo.txt');
  try
    ExtractTemporaryFile('ponteproj.exe');
  except
    { Sem o programa novo não há cópia consistente: copiar o arquivo do acervo
      e depois o -wal pode dar uma cópia velha se um checkpoint acontecer entre
      os dois. Falhar aqui é falha da cópia, como qualquer outra. }
    Log('Não foi possível extrair o programa novo para a cópia de segurança: ' + GetExceptionMessage);
    Detail := 'Não foi possível extrair o programa de cópia (um antivírus pode ter bloqueado): ' + GetExceptionMessage;
    exit;
  end;
  DeleteFile(ReportFile);
  Params := '--copia-antes-de-atualizar --rotulo v{#AppVersion} --resultado "' + ReportFile + '"';
  if not Exec(Exe, Params, '', SW_HIDE, ewWaitUntilTerminated, ResultCode) then
  begin
    Log('O programa novo não pôde ser executado para a cópia de segurança: ' + SysErrorMessage(ResultCode));
    Detail := 'Não foi possível executar o programa de cópia (um antivírus pode ter bloqueado): ' + SysErrorMessage(ResultCode);
    exit;
  end;
  Status := ReadBackupReport(ReportFile, ReportDetail);
  Log('Cópia de segurança do acervo: código ' + IntToStr(ResultCode) + ', ' + Status + ' ' + ReportDetail);
  if ResultCode = 0 then
  begin
    Result := True;
    if Status = 'ok' then
      BackupPath := ReportDetail;
  end
  else
  begin
    Detail := ReportDetail;
    if Detail = '' then
      Detail := 'O programa de cópia terminou com o código ' + IntToStr(ResultCode) + '.';
  end;
end;

{ Antes de qualquer arquivo ser trocado: sem cópia conferida do acervo, a
  instalação não segue. Na instalação silenciosa não há pergunta: termina com
  código 8. Na interativa, a pessoa escolhe entre cancelar (o padrão) e seguir
  sem cópia. }
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Detail, Message: String;
  Labels: TArrayOfString;
begin
  Result := '';
  if BackupLibrary(Detail) then
  begin
    if BackupPath <> '' then
      Log('Cópia de segurança do acervo: ' + BackupPath);
    exit;
  end;
  Message := 'Não foi possível fazer a cópia de segurança do acervo antes de atualizar.' + #13#10#13#10
    + Detail + #13#10#13#10
    + 'Onde olhar: a pasta backups do acervo (%LOCALAPPDATA%\PonteProj\backups, ou a pasta backups dentro da pasta definida em PONTEPROJ_DATA_DIR): confira o espaço em disco e a permissão de gravação. '
    + 'O Ponte instalado e o acervo não foram alterados.';
  Log('Falha na cópia de segurança do acervo: ' + Detail);
  if WizardSilent then
  begin
    Result := Message;
    exit;
  end;
  SetArrayLength(Labels, 2);
  Labels[0] := 'Cancelar a instalação (recomendado)';
  Labels[1] := 'Continuar sem cópia de segurança';
  if TaskDialogMsgBox('A cópia de segurança do acervo falhou', Message, mbError, MB_YESNO, Labels, 0) <> IDNO then
    Result := 'A instalação foi cancelada. O Ponte instalado e o acervo continuam como estavam.'
  else
    Log('A pessoa escolheu continuar sem cópia de segurança do acervo.');
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
  if CurStep = ssPostInstall then
  begin
    WriteHostManifest;
    RemoveOldVersions;
  end;
end;
