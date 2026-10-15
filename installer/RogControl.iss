; RogControl installer (Inno Setup 6)
; Requires: .NET 10 Desktop Runtime on the target machine (checked below).
; The WinAppSDK runtime is bundled (WindowsAppSDKSelfContained=true), so the
; Windows App Runtime does not need to be pre-installed.

#define MyAppName "RogControl"
#define MyAppVersion "1.0.0"
#define MyAppPublisher "RogControl"
#define MyAppExeName "RogControl.exe"

; Release x64 publish output with the WinAppSDK runtime embedded.
#define MyAppPublishDir "..\bin\x64\Release\net10.0-windows10.0.22621.0\win-x64\publish"

[Setup]
AppId={{3F7D9C2E-4A18-4E9B-9C6A-2F8B5D1E7A31}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={autopf}\{#MyAppName}
DisableProgramGroupPage=yes
ArchitecturesInstallIn64BitMode=x64
OutputDir=..\artifacts
OutputBaseFilename=RogControl-Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}

[Languages]
Name: "chinesesimp"; MessagesFile: "compiler:Default.isl,compiler:Languages\ChineseSimplified.isl"

[Tasks]
Name: "autostart"; Description: "开机自动启动"; GroupDescription: "启动选项:";

[Files]
Source: "{#MyAppPublishDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: autostart

[Registry]
; Optional auto-start via HKCU Run key (removed on uninstall).
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; \
  ValueType: string; ValueName: "{#MyAppName}"; \
  ValueData: """{app}\{#MyAppExeName}"""; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "启动 {#MyAppName}"; \
  Flags: nowait postinstall skipifsilent

[Code]
function IsDotNet10DesktopInstalled: Boolean;
var
  Key: string;
  Versions: TArrayOfString;
  I: Integer;
begin
  Result := False;
  Key := 'SOFTWARE\dotnet\Setup\InstalledVersions\x64\Microsoft.WindowsDesktop.App';
  if RegValueExists(HKLM, Key, 'Version') then
  begin
    if RegQueryStringValue(HKLM, Key, 'Version', Versions[0]) then
    begin
      // Look for a 10.x version string.
      if Pos('10.', Versions[0]) = 1 then
      begin
        Result := True;
        Exit;
      end;
    end;
  end;
  // Fallback: scan all installed versions under the same key name list.
  if RegGetSubkeyNames(HKLM, 'SOFTWARE\dotnet\Setup\InstalledVersions\x64', Versions) then
  begin
    for I := 0 to GetArrayLength(Versions) - 1 do
    begin
      if Pos('Microsoft.WindowsDesktop.App', Versions[I]) > 0 then
      begin
        Key := 'SOFTWARE\dotnet\Setup\InstalledVersions\x64\' + Versions[I];
        if RegQueryStringValue(HKLM, Key, 'Version', Versions[I]) then
        begin
          if Pos('10.', Versions[I]) = 1 then
          begin
            Result := True;
            Exit;
          end;
        end;
      end;
    end;
  end;
end;

procedure InitializeWizard;
begin
  if not IsDotNet10DesktopInstalled then
  begin
    MsgBox('未检测到 .NET 10 桌面运行时。' + #13#10 + #13#10 +
           '请先安装 ".NET Desktop Runtime 10.x"（下载页面：' + #13#10 +
           'https://dotnet.microsoft.com/download/dotnet/10.0），' + #13#10 +
           '然后重新运行本安装程序。', mbInformation, MB_OK);
  end;
end;
