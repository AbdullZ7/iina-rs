; IINA-RS Windows installer (Inno Setup 6).
;   iscc /DAppVersion=0.5.0 /DSrcDir=<folder with iina-rs.exe + libmpv-2.dll> /DOutDir=dist scripts\installer.iss
; Installs for the current user by default (no admin); "Install for all
; users" is offered in a dialog. Uninstall leaves the user's settings.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SrcDir
  #define SrcDir "..\target\release"
#endif
#ifndef OutDir
  #define OutDir "..\dist"
#endif

[Setup]
AppId={{7C2B5D1E-4A3F-4E8B-9D61-1B7A3C9E5F20}
AppName=IINA-RS
AppVersion={#AppVersion}
AppVerName=IINA-RS {#AppVersion}
AppPublisher=IINA-RS
AppPublisherURL=https://github.com/AbdullZ7/iina-rs
AppSupportURL=https://github.com/AbdullZ7/iina-rs/issues
AppUpdatesURL=https://github.com/AbdullZ7/iina-rs/releases
DefaultDirName={autopf}\IINA-RS
DefaultGroupName=IINA-RS
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutDir}
OutputBaseFilename=IINA-RS-{#AppVersion}-windows-x64-setup
SetupIconFile=..\assets\iina-rs.ico
UninstallDisplayIcon={app}\iina-rs.exe
UninstallDisplayName=IINA-RS
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
ChangesAssociations=yes

; Arabic ships with Inno Setup 6.4+ (Languages\Arabic.isl).
#define ArabicIsl AddBackslash(CompilerPath) + "Languages\Arabic.isl"

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
#if FileExists(ArabicIsl)
Name: "ar"; MessagesFile: "{#ArabicIsl}"
#endif

[CustomMessages]
en.AssocTask=Add IINA-RS to "Open with" for video and audio files
en.LaunchApp=Launch IINA-RS
#if FileExists(ArabicIsl)
ar.AssocTask=إضافة IINA-RS إلى «فتح باستخدام» لملفات المرئيات والسمعيات
ar.LaunchApp=شغّل IINA-RS
#endif

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "assoc"; Description: "{cm:AssocTask}"

[Files]
Source: "{#SrcDir}\iina-rs.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SrcDir}\libmpv-2.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SrcDir}\yt-dlp.exe"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

[Icons]
Name: "{autoprograms}\IINA-RS"; Filename: "{app}\iina-rs.exe"
Name: "{autodesktop}\IINA-RS"; Filename: "{app}\iina-rs.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\iina-rs.exe"; Parameters: "--register-file-types"; Tasks: assoc; Flags: runhidden waituntilterminated
Filename: "{app}\iina-rs.exe"; Description: "{cm:LaunchApp}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Removes "Open with" only if it points at this installed copy.
Filename: "{app}\iina-rs.exe"; Parameters: "--unregister-file-types"; Flags: runhidden waituntilterminated; RunOnceId: "UnregisterTypes"
