; The Windows installer, built in CI by Inno Setup:
;   iscc /DVersion=X.Y.Z /DBinary=path\to\client.exe assets\windows\rouillo.iss
; Installed per user, without admin rights, so that the in-game updater can
; replace the executable in place.

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Binary
  #define Binary "..\..\target\release\client.exe"
#endif

[Setup]
; The game finds its uninstall entry by this id (client/src/windows.rs).
AppId={{89402259-6417-4E94-9C00-13FA4F6030E6}
AppName=Rouillo
AppVersion={#Version}
AppPublisher=Priax
AppPublisherURL=https://github.com/Priax/Rouillo
DefaultDirName={autopf}\Rouillo
DefaultGroupName=Rouillo
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile=..\puyo_puyo_icon.ico
UninstallDisplayIcon={app}\Rouillo.exe
OutputDir=..\..
OutputBaseFilename=rouillo-windows-x86_64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ShowLanguageDialog=auto
CloseApplications=yes

[Languages]
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#Binary}"; DestDir: "{app}"; DestName: "Rouillo.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Rouillo"; Filename: "{app}\Rouillo.exe"
Name: "{autodesktop}\Rouillo"; Filename: "{app}\Rouillo.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Rouillo.exe"; Description: "{cm:LaunchProgram,Rouillo}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: files; Name: "{app}\Rouillo.new"
Type: files; Name: "{app}\.Rouillo.*.exe"
