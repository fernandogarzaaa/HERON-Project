; Inno Setup script for the Heron installer. Per-user install: no administrator rights needed.
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

[Setup]
AppId={{6F0B7C1E-4E2A-4D0B-9A53-2C8D5E7A91B4}
AppName=Heron
AppVersion={#AppVersion}
AppPublisher=Heron Project
AppPublisherURL=https://github.com/fernandogarzaaa/HERON-Project
DefaultDirName={localappdata}\Programs\Heron
DefaultGroupName=Heron
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\..\dist
OutputBaseFilename=Heron-Setup-windows-x64
SetupIconFile=..\icons\icon.ico
UninstallDisplayIcon={app}\Heron.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
LicenseFile=..\..\LICENSE

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "..\..\dist\Heron\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{autoprograms}\Heron"; Filename: "{app}\Heron.exe"
Name: "{autodesktop}\Heron"; Filename: "{app}\Heron.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Heron.exe"; Description: "Start Heron"; Flags: nowait postinstall skipifsilent
