; Paperloom setup .exe (Inno Setup 6). Built by .github/workflows/package.yml:
;
;   iscc /DAppVersion=0.2.0 /DStage=C:\stage /DOutputDir=C:\out paperloom.iss
;
; Installs per-user by default (no admin prompt, so in-app updates are one
; click); the first page lets the user switch to an all-users install.
; The AppId must never change: it's how a newer setup finds the old install.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Stage
  #define Stage "stage"
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

[Setup]
AppId={{a8ee3a86-4085-4dc2-ae0b-d47b3ca158c3}
AppName=Paperloom
AppVersion={#AppVersion}
AppVerName=Paperloom {#AppVersion}
AppPublisher=Amanullah Behlim
AppPublisherURL=https://github.com/theamanullahdev/paperloom
AppSupportURL=https://github.com/theamanullahdev/paperloom/issues
AppUpdatesURL=https://github.com/theamanullahdev/paperloom/releases
DefaultDirName={autopf}\Paperloom
DefaultGroupName=Paperloom
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile=..\..\LICENSE
SetupIconFile=paperloom.ico
UninstallDisplayIcon={app}\paperloom-gui.exe
OutputDir={#OutputDir}
OutputBaseFilename=paperloom-{#AppVersion}-windows-x86_64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
; The engine has no top-level window for Restart Manager to ask nicely,
; so it's stopped explicitly in PrepareToInstall below.
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Stage}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{autoprograms}\Paperloom"; Filename: "{app}\paperloom-gui.exe"
Name: "{autodesktop}\Paperloom"; Filename: "{app}\paperloom-gui.exe"; Tasks: desktopicon

[Registry]
; Removes the "start at login" entry the app may have created.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueName: "Paperloom"; Flags: uninsdeletevalue dontcreatekey

[Run]
Filename: "{app}\paperloom-gui.exe"; Description: "{cm:LaunchProgram,Paperloom}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM paperloom-engine.exe"; Flags: runhidden; RunOnceId: "StopEngine"

[Code]
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ResultCode: Integer;
begin
  { An upgrade can't replace a running engine; stop it first. }
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM paperloom-engine.exe', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Result := '';
end;
