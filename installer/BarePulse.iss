#define MyAppName "BarePulse"
#define MyAppPublisher "Tamás Németh"
#define MyAppExeName "BarePulse.exe"
#define MyAppVersion GetEnv("BAREPULSE_PACKAGE_VERSION")

#if MyAppVersion == ""
  #error BAREPULSE_PACKAGE_VERSION is not set
#endif

[Setup]
AppId={{77A9313A-4C74-4F3A-9A64-6E40240743A8}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL=https://github.com/Nemeth-Tamas/BarePulse
AppSupportURL=https://github.com/Nemeth-Tamas/BarePulse/issues
AppUpdatesURL=https://github.com/Nemeth-Tamas/BarePulse/releases

DefaultDirName={localappdata}\BarePulse
DefaultGroupName=BarePulse
DisableProgramGroupPage=yes

PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible

OutputDir=..\dist
OutputBaseFilename=BarePulse-v{#MyAppVersion}-Setup

Compression=lzma2
SolidCompression=yes
WizardStyle=modern

UninstallDisplayName=BarePulse
UninstallDisplayIcon={app}\BarePulse.exe

CloseApplications=yes
RestartApplications=no
UsePreviousTasks=yes

[Tasks]
Name: "startmenu"; Description: "Create a Start Menu shortcut"; GroupDescription: "Shortcuts:"
Name: "startup"; Description: "Start BarePulse with Windows"; GroupDescription: "Startup:"; Flags: unchecked

[Files]
Source: "..\dist\BarePulse\BarePulse.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\BarePulse\devices\manifest.toml"; DestDir: "{app}\devices"; Flags: ignoreversion

[Icons]
Name: "{group}\BarePulse"; Filename: "{app}\BarePulse.exe"; WorkingDir: "{app}"; Tasks: startmenu
Name: "{group}\Uninstall BarePulse"; Filename: "{uninstallexe}"; Tasks: startmenu
Name: "{autostartup}\BarePulse"; Filename: "{app}\BarePulse.exe"; WorkingDir: "{app}"; Tasks: startup

[Run]
Filename: "{app}\BarePulse.exe"; Description: "Launch BarePulse"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: files; Name: "{app}\barepulse.toml"
Type: filesandordirs; Name: "{app}\devices"
Type: dirifempty; Name: "{app}"