; SPDX-License-Identifier: GPL-3.0-only
; Smorzo installer (Inno Setup 6). Built by GitHub Actions on a version tag:
;   ISCC.exe /DAppVersion=0.1.0 /DBinDir=<dir with smorzo.exe> /DIconFile=<smorzo.ico> installer\smorzo.iss
;
; - Per-user install in %LOCALAPPDATA%\Programs\Smorzo: no admin rights, and the in-app
;   updater can replace smorzo.exe there without admin rights too.
; - Start menu entry, optional desktop icon, clean uninstall.
; - Uninstall removes Smorzo's "Start with Windows" entry but keeps your settings in
;   %APPDATA%\Smorzo, so reinstalling brings them back.
; - VB-Cable is never bundled; Smorzo shows the official link if it's missing.
; - Smorzo used to be called Sotto (same AppId, so this is an upgrade). Installing over
;   Sotto removes the old program file and its shortcuts; the settings in %APPDATA%\Sotto
;   are kept, and Smorzo copies them over on its first start.

#ifndef AppVersion
  #error AppVersion is required (/DAppVersion=x.y.z)
#endif
#ifndef BinDir
  #error BinDir is required (/DBinDir=...)
#endif
#ifndef IconFile
  #error IconFile is required (/DIconFile=...)
#endif
#define RepoDir AddBackslash(SourcePath) + ".."

[Setup]
; Never change AppId: Windows uses it to recognise upgrades and the uninstall entry.
AppId={{F62C2347-9DBC-47BC-812D-271B6AC542FD}
AppName=Smorzo
AppVersion={#AppVersion}
AppVerName=Smorzo {#AppVersion}
AppPublisher=hamadsh2
AppPublisherURL=https://github.com/hamadsh2/Smorzo
AppSupportURL=https://github.com/hamadsh2/Smorzo/issues
AppUpdatesURL=https://github.com/hamadsh2/Smorzo/releases
AppCopyright=Smorzo contributors, GPL-3.0-only
PrivilegesRequired=lowest
DefaultDirName={userpf}\Smorzo
; Don't reuse an old Sotto folder: always install to Programs\Smorzo.
UsePreviousAppDir=no
DisableDirPage=auto
DefaultGroupName=Smorzo
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputBaseFilename=Smorzo-{#AppVersion}-setup
OutputDir={#RepoDir}\dist
SetupIconFile={#IconFile}
UninstallDisplayIcon={app}\smorzo.exe
UninstallDisplayName=Smorzo
VersionInfoVersion={#AppVersion}
VersionInfoProductName=Smorzo
VersionInfoDescription=Smorzo installer
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; Smorzo is closed by the [Code] below before files are replaced.
CloseApplications=no

[Tasks]
Name: "desktopicon"; Description: "Create a desktop icon"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "{#BinDir}\smorzo.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoDir}\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "{#RepoDir}\assets\THIRD_PARTY_LICENSES.txt"; DestDir: "{app}"; Flags: ignoreversion

[InstallDelete]
; What an old Sotto install left (v0.4.0 and older). Only these exact files.
Type: files; Name: "{userpf}\Sotto\sotto.exe"
Type: files; Name: "{userpf}\Sotto\LICENSE.txt"
Type: files; Name: "{userpf}\Sotto\THIRD_PARTY_LICENSES.txt"
Type: dirifempty; Name: "{userpf}\Sotto"
Type: files; Name: "{userprograms}\Sotto\Sotto.lnk"
Type: dirifempty; Name: "{userprograms}\Sotto"
Type: files; Name: "{userdesktop}\Sotto.lnk"

[Icons]
Name: "{group}\Smorzo"; Filename: "{app}\smorzo.exe"
Name: "{userdesktop}\Smorzo"; Filename: "{app}\smorzo.exe"; Tasks: desktopicon

[Registry]
; Only removes Smorzo's own "Start with Windows" value on uninstall; creates nothing.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "Smorzo"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "Sotto"; Flags: uninsdeletevalue

[Run]
Filename: "{app}\smorzo.exe"; Description: "Start Smorzo"; Flags: nowait postinstall skipifsilent

[Code]
// Smorzo keeps running in the tray when its window is closed, so stop it before
// replacing or removing its files. Its settings are saved half a second after each
// change, so nothing is lost. Only smorzo.exe is affected.
procedure StopSmorzo();
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM smorzo.exe /F', '', SW_HIDE,
    ewWaitUntilTerminated, Code);
  // Smorzo's old name, so an upgrade from Sotto can replace its files.
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM sotto.exe /F', '', SW_HIDE,
    ewWaitUntilTerminated, Code);
  Sleep(500);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopSmorzo();
  Result := '';
end;

function InitializeUninstall(): Boolean;
begin
  StopSmorzo();
  Result := True;
end;
