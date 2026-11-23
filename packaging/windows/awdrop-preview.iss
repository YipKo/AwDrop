; AwDrop WinUI desktop preview. No driver or service is installed.
#ifndef Version
  #error Compile using scripts\build-windows-installer.ps1
#endif
[Setup]
AppId={{D7DF11A9-DA90-496E-9873-3B381DBF9141}
AppName=AwDrop Desktop Preview
AppVersion={#Version}
AppPublisher=AwDrop
DefaultDirName={localappdata}\Programs\AwDrop Preview
DefaultGroupName=AwDrop Preview
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.19041
WizardStyle=modern
SetupIconFile=..\..\winawdl\app\winawdl.ico
UninstallDisplayIcon={app}\AwDrop.Windows.exe
OutputDir={#Output}
OutputBaseFilename=AwDrop_{#Version}_x64-setup
Compression=lzma2
SolidCompression=yes
VersionInfoVersion={#Version}
VersionInfoDescription=AwDrop Desktop Preview Setup
InfoBeforeFile=awdrop-preview.txt
[InstallDelete]
Type: files; Name: "{app}\awdrop.exe"
[Files]
Source: "{#AppDir}\*"; DestDir: "{app}"; Excludes: "*.pdb"; Flags: ignoreversion recursesubdirs
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
[Icons]
Name: "{autoprograms}\AwDrop Preview"; Filename: "{app}\AwDrop.Windows.exe"; WorkingDir: "{app}"
[Run]
Filename: "{app}\AwDrop.Windows.exe"; Description: "Launch AwDrop Desktop Preview"; Flags: postinstall nowait skipifsilent
