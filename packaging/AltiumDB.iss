; Inno Setup 6 script for AltiumDB
; Build: 1) cargo build --release  2) ISCC.exe packaging\AltiumDB.iss

#define AppName "AltiumDB"
#define AppVersion GetFileVersion("..\\target\\release\\AltiumDB.exe")
#define AppPublisher "Selyutin Anton"

[Setup]
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
OutputDir=dist
OutputBaseFilename={#AppName}-Setup-{#AppVersion}
Compression=lzma2/max
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
SetupIconFile=..\icon.ico
WizardStyle=modern
UninstallDisplayIcon={app}\{#AppName}.exe

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\target\release\{#AppName}.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppName}.exe"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppName}.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppName}.exe"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[Code]
const
  PythonDownloadUrl = 'https://www.python.org/downloads/windows/';
  AltiumMonkeyPackage = 'https://github.com/wavenumber-eng/altium_monkey/archive/refs/heads/main.zip';

function RunHidden(const FileName, Parameters: String): Boolean;
var
  ExitCode: Integer;
begin
  Result := Exec(FileName, Parameters, '', SW_HIDE, ewWaitUntilTerminated, ExitCode)
    and (ExitCode = 0);
end;

function FindPython(var PythonExe: String): Boolean;
begin
  PythonExe := 'python';
  Result := RunHidden(PythonExe, '-c "import sys; print(sys.version)"');
end;

function PythonHasAltiumMonkey(const PythonExe: String): Boolean;
var
  ImportArgs: String;
begin
  ImportArgs := '-c "import altium_monkey"';
  Result := RunHidden(PythonExe, ImportArgs);
end;

function OfferPythonDownload: Boolean;
var
  ErrorCode: Integer;
begin
  Result := False;
  if MsgBox(
       'Python 3 is required for Symbol and Footprint previews, but it was not found.'#13#10#13#10 +
       'Open the official Python download page now?',
       mbError, MB_YESNO) = IDYES then begin
    ShellExec('open', PythonDownloadUrl, '', '', SW_SHOWNORMAL, ewNoWait, ErrorCode);
  end;
end;

function InstallAltiumMonkey(const PythonExe: String): Boolean;
var
  PipArgs: String;
begin
  PipArgs := '-m pip install ' + AltiumMonkeyPackage;
  Result := RunHidden(PythonExe, PipArgs);
  if not Result then
    MsgBox(
      'The Altium Monkey Python package could not be installed.'#13#10#13#10 +
      'Install it manually with:'#13#10 + PythonExe + ' ' + PipArgs,
      mbError, MB_OK);
end;

function CheckRuntimePrerequisites: Boolean;
var
  PythonExe: String;
begin
  Result := False;
  if not FindPython(PythonExe) then begin
    OfferPythonDownload;
    Exit;
  end;

  if not PythonHasAltiumMonkey(PythonExe) then begin
    if MsgBox(
         'The Python package altium_monkey is required for Symbol and Footprint previews.'#13#10#13#10 +
         'Install it now from the official GitHub repository?',
         mbError, MB_YESNO) <> IDYES then
      Exit;
    if not InstallAltiumMonkey(PythonExe) then
      Exit;
    if not PythonHasAltiumMonkey(PythonExe) then begin
      MsgBox(
        'The altium_monkey package is still unavailable. Installation cannot continue.',
        mbError, MB_OK);
      Exit;
    end;
  end;
  Result := True;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if CurPageID = wpSelectDir then
    Result := CheckRuntimePrerequisites;
end;
