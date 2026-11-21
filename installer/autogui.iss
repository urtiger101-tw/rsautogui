; AutoGui per-user installer. This is a thin wrapper: install.ps1 owns the
; program files, SHA-256 verification, host configuration and the manifest.
; Inno writes nothing into {app} (its uninstaller lives in {app}.setup) so that
; install.ps1 always sees an empty program directory.
;
; Compile with scripts\package.ps1 -Installer, or manually:
;   ISCC /DAppVersion=0.1.0 /DPackageDir=<payload dir> /DOutputDir=<dir> installer\autogui.iss
; Tests may override /DAppIdGuid=<GUID> so their registration cannot collide.

#ifndef AppVersion
  #error AppVersion must be defined with /DAppVersion=<version>
#endif
#ifndef PackageDir
  #error PackageDir must be defined with /DPackageDir=<payload directory>
#endif
#ifndef OutputDir
  #error OutputDir must be defined with /DOutputDir=<directory>
#endif
#ifndef AppIdGuid
  #define AppIdGuid "25D994F3-34C0-4C55-AAAC-4700E3289160"
#endif

[Setup]
; Doubled brace is Inno's escape for a literal "{" that opens the GUID.
AppId={{{#AppIdGuid}}
AppName=AutoGui
AppVersion={#AppVersion}
AppPublisher=AutoGui
DefaultDirName={localappdata}\Programs\AutoGui
UsePreviousAppDir=no
UninstallFilesDir={app}.setup
PrivilegesRequired=lowest
DisableDirPage=yes
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=no
SetupLogging=yes
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
OutputDir={#OutputDir}
OutputBaseFilename=AutoGui-{#AppVersion}-setup

[Tasks]
; Hierarchical task names: a child is selected by its parent-qualified name and
; selecting a child also selects its parent. Blank /TASKS means program only.
Name: "skill"; Description: "Install Skill for:"; Flags: unchecked
Name: "skill\codex"; Description: "Codex"; Flags: unchecked
Name: "skill\claude"; Description: "Claude Code"; Flags: unchecked
Name: "skill\opencode"; Description: "OpenCode"; Flags: unchecked
Name: "skill\agy"; Description: "AGY App / CLI"; Flags: unchecked
Name: "mcp"; Description: "Register MCP server for:"; Flags: unchecked
Name: "mcp\codex"; Description: "Codex"; Flags: unchecked
Name: "mcp\claude"; Description: "Claude Code"; Flags: unchecked
Name: "mcp\claude-desktop"; Description: "Claude Desktop"; Flags: unchecked
Name: "mcp\opencode"; Description: "OpenCode"; Flags: unchecked
Name: "mcp\agy"; Description: "AGY App / CLI"; Flags: unchecked

[Files]
; Staged only for install.ps1; deleteafterinstall removes the staging copy.
Source: "{#PackageDir}\*"; DestDir: "{tmp}\pkg"; Flags: recursesubdirs createallsubdirs ignoreversion deleteafterinstall

[Code]
var
  InstallFailed: Boolean;

function ManifestPath(): String;
begin
  Result := ExpandConstant('{app}\.install\manifest.json');
end;

function RunPackageScript(const ScriptPath, Arguments: String; var ResultCode: Integer): Boolean;
begin
  // ExecAndLogOutput (Inno Setup 6.4+) copies the script's output into the
  // Setup/uninstall log so install.ps1 failures can be diagnosed.
  Result := ExecAndLogOutput(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + ScriptPath + '" ' + Arguments,
    '', SW_HIDE, ewWaitUntilTerminated, ResultCode, nil);
end;

procedure AddHost(var Csv: String; const Group, Name: String);
begin
  if WizardIsTaskSelected(Group + '\' + Name) then
  begin
    if Csv <> '' then Csv := Csv + ',';
    Csv := Csv + Name;
  end;
end;

function SelectedHosts(const Group: String): String;
begin
  Result := '';
  AddHost(Result, Group, 'codex');
  AddHost(Result, Group, 'claude');
  // Claude Desktop reads MCP servers only; it has no Skill task.
  if Group = 'mcp' then AddHost(Result, Group, 'claude-desktop');
  AddHost(Result, Group, 'opencode');
  AddHost(Result, Group, 'agy');
end;

// A previous uninstall.ps1 keeps files whose hashes changed after install
// (for example locally replaced program files) together with the manifest,
// and removes itself. Such a directory blocks install.ps1. With consent
// (or /PRESERVELEFTOVERS=1 in silent mode) the whole directory is renamed to
// a sibling {app}.preserved-<timestamp>; nothing is deleted.
function PreserveLeftovers(): String;
var
  App, Target: String;
begin
  Result := '';
  App := ExpandConstant('{app}');
  Target := App + '.preserved-' + GetDateTimeString('yyyymmdd-hhnnss', #0, #0);
  if (ExpandConstant('{param:PRESERVELEFTOVERS|0}') <> '1') and
     (SuppressibleMsgBox('A previous AutoGui installation left files that were modified after it was installed ' +
       '(for example replaced program files). They were kept and block a clean install.' + #13#10#13#10 +
       'Move the folder' + #13#10 + App + #13#10 + 'to' + #13#10 + Target + #13#10 +
       'and continue? Nothing is deleted.', mbConfirmation, MB_YESNO, IDNO) <> IDYES) then
  begin
    Result := 'Setup stopped. The kept files are still in ' + App + '. Move or remove that folder, then run Setup again.';
    Exit;
  end;
  if RenameFile(App, Target) then
    Log('Moved leftover installation to ' + Target)
  else
    Result := 'Could not move ' + App + ' to ' + Target + '. ' +
      'Close agents and MCP hosts that use autogui-control.exe, then run Setup again.';
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ResultCode: Integer;
  Uninstaller: String;
begin
  Result := '';
  // Upgrade path: an existing manifest is removed by its own uninstall.ps1
  // first. If files remain (modified files, or uninstall.ps1 already gone
  // after an earlier partial uninstall), offer to move them aside.
  if FileExists(ManifestPath()) then
  begin
    Uninstaller := ExpandConstant('{app}\uninstall.ps1');
    if FileExists(Uninstaller) then
    begin
      if (not RunPackageScript(Uninstaller, '-InstallDir "' + ExpandConstant('{app}') + '"', ResultCode))
         or (ResultCode <> 0) then
        Log('Previous uninstall.ps1 did not complete; exit code ' + IntToStr(ResultCode));
    end
    else
      Log('Previous uninstall.ps1 is missing; an earlier uninstall was partial.');
    if FileExists(ManifestPath()) then
      Result := PreserveLeftovers();
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Arguments, SkillHosts, McpHosts: String;
  ResultCode: Integer;
begin
  if CurStep = ssPostInstall then
  begin
    SkillHosts := SelectedHosts('skill');
    McpHosts := SelectedHosts('mcp');
    Arguments := '-NonInteractive -InstallDir "' + ExpandConstant('{app}') + '"';
    if SkillHosts <> '' then Arguments := Arguments + ' -SkillHosts "' + SkillHosts + '"';
    if McpHosts <> '' then Arguments := Arguments + ' -McpHosts "' + McpHosts + '"';
    if (not RunPackageScript(ExpandConstant('{tmp}\pkg\install.ps1'), Arguments, ResultCode)) or (ResultCode <> 0) then
    begin
      InstallFailed := True;
      SuppressibleMsgBox('AutoGui program installation failed. Setup log: ' + ExpandConstant('{log}'),
        mbCriticalError, MB_OK, IDOK);
    end;
  end;
end;

function GetCustomSetupExitCode(): Integer;
begin
  if InstallFailed then Result := 1 else Result := 0;
end;

procedure DeinitializeSetup();
var
  ResultCode: Integer;
begin
  // install.ps1 already rolled back its own files; this removes the Inno
  // uninstaller data left by a partially completed setup.
  if InstallFailed and FileExists(ExpandConstant('{uninstallexe}')) then
    Exec(ExpandConstant('{uninstallexe}'), '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART', '',
      SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  ResultCode: Integer;
begin
  if (CurUninstallStep = usUninstall) and FileExists(ManifestPath()) then
  begin
    // Only files still matching their recorded hashes are removed. Anything
    // changed afterwards, and recovery data, stays in place; the uninstaller
    // continues so that Inno's own registration is still removed.
    if (not RunPackageScript(ExpandConstant('{app}\uninstall.ps1'), '-InstallDir "' + ExpandConstant('{app}') + '"', ResultCode))
       or (ResultCode <> 0) or FileExists(ManifestPath()) then
      SuppressibleMsgBox('Some AutoGui files were modified or could not be removed and were kept in ' +
        ExpandConstant('{app}') + '.', mbInformation, MB_OK, IDOK);
  end;
end;
