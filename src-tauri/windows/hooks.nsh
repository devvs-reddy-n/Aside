; Aside's prebuilt inference library uses Microsoft's shared C++ runtime.
; Keep it independent from Aside: uninstalling Aside must never remove it.
!define ASIDE_HOOK_DIR "${__FILEDIR__}"
!include "WordFunc.nsh"

!macro NSIS_HOOK_PREINSTALL
  SetRegView 64
  ReadRegStr $0 HKLM "SOFTWARE\Microsoft\Windows NT\CurrentVersion" "CurrentBuildNumber"
  ${If} $0 < 19041
    MessageBox MB_ICONSTOP "Aside needs Windows 10 version 2004 or newer, or Windows 11. Update Windows, then run Aside Setup again."
    Abort
  ${EndIf}
  ; Microsoft records the runtime versions in the 32-bit registry view.
  SetRegView 32
  ReadRegStr $0 HKLM "SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64" "Version"
  SetRegView 64
  ; Microsoft's Version value begins with v. Missing values compare as zero.
  StrCpy $0 $0 "" 1
  ${VersionCompare} $0 "14.51.36247.0" $1
  ${If} $1 == 2
    InitPluginsDir
    SetOutPath "$PLUGINSDIR"
    File /oname=vc_redist.x64.exe "${ASIDE_HOOK_DIR}/../target.noindex/windows-deps/vc_redist.x64.exe"
    DetailPrint "Installing Microsoft's C++ runtime. Windows may ask for administrator approval."
    ClearErrors
    ExecShellWait "runas" "$PLUGINSDIR\vc_redist.x64.exe" "/install /passive /norestart" SW_SHOWNORMAL $0
    ${If} ${Errors}
      MessageBox MB_ICONSTOP "Microsoft's C++ runtime could not start. Run this installer again and allow the Windows approval prompt."
      Abort
    ${EndIf}
    ${If} $0 != 0
    ${AndIf} $0 != 3010
    ${AndIf} $0 != 1638
      MessageBox MB_ICONSTOP "Microsoft's C++ runtime could not be installed (code $0). Install the x64 Microsoft Visual C++ Redistributable, then run Aside Setup again."
      Abort
    ${EndIf}
    ${If} $0 == 3010
      SetRebootFlag true
    ${EndIf}
    Delete "$PLUGINSDIR\vc_redist.x64.exe"
  ${EndIf}
  ; Tauri copies Aside immediately after this hook. Restore its destination.
  SetOutPath "$INSTDIR"
!macroend
