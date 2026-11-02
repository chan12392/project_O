; project_O installer hooks (tauri.conf.json > bundle > windows > nsis > installerHooks).
;
; Tauri's per-user default install folder is $LOCALAPPDATA\<productName>, which for
; project_O is %LOCALAPPDATA%\project_O: O's state folder (o-core store::default_home(),
; protected by policy). App files must never share it, so the default is moved to
; %LOCALAPPDATA%\Programs\project_O (the usual per-user program folder). A folder given
; with /D=... or kept from an earlier install is left alone, unless it is the state folder.
;
; This file is included before the installer defines PRODUCTNAME, so the names are spelled out.

!define PROJECT_O_STATE_DIR "$LOCALAPPDATA\project_O"
!define PROJECT_O_APP_DIR "$LOCALAPPDATA\Programs\project_O"

Function ProjectOAvoidStateDir
  ${If} $INSTDIR == "${PROJECT_O_STATE_DIR}"
    StrCpy $INSTDIR "${PROJECT_O_APP_DIR}"
  ${EndIf}
FunctionEnd

; Interactive installs: fix the folder before the directory page shows it.
!define MUI_CUSTOMFUNCTION_GUIINIT ProjectOAvoidStateDir

; Silent installs (/S) never run the GUI init: fix it again before any file is copied.
!macro NSIS_HOOK_PREINSTALL
  Call ProjectOAvoidStateDir
  SetOutPath $INSTDIR
  !insertmacro CheckIfAppIsRunning "$INSTDIR\o.exe" "Project O CLI"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro CheckIfAppIsRunning "$INSTDIR\project_O.exe" "Project O"
  !insertmacro CheckIfAppIsRunning "$INSTDIR\o.exe" "Project O CLI"
  StrCpy $R5 $DeleteAppDataCheckboxState
  ; Own all data removal: Tauri's unchecked recursive paths must not run afterwards.
  StrCpy $DeleteAppDataCheckboxState 0
  ${GetOptions} $CMDLINE "/PURGE" $R0
  ${IfNot} ${Errors}
    StrCpy $R5 1
  ${EndIf}
  ${If} $R5 == 1
    StrCpy $R6 ""
    ${GetOptions} $CMDLINE "/DATADRYRUN" $R0
    ${IfNot} ${Errors}
      StrCpy $R6 "--dry-run"
    ${EndIf}
    StrCpy $R7 ""
    ${GetOptions} $CMDLINE "/TESTDATA=" $R0
    ${IfNot} ${Errors}
      StrCpy $R7 '--test-id "$R0"'
    ${EndIf}
    nsExec::ExecToStack '"$INSTDIR\o-uninstall-data.exe" $R6 $R7'
    Pop $0
    Pop $1
    ${If} $0 != 0
      DetailPrint $1
      ${If} $TEMP != ""
        FileOpen $R8 "$TEMP\project_O-uninstall-error.log" w
        FileWrite $R8 "$0: $1$\r$\n"
        FileClose $R8
      ${EndIf}
      IfSilent +2
        MessageBox MB_OK|MB_ICONSTOP "앱 데이터를 정리하지 못했습니다. 실행 중인 작업을 닫고 다시 시도해 주세요.$\r$\n$1"
      SetErrorLevel 1
      Abort
    ${EndIf}
    DetailPrint $1
    ${If} $R6 == ""
    ${AndIf} $R7 == ""
      DeleteRegKey HKCU "Software\Project O\project_O"
    ${EndIf}
  ${EndIf}
!macroend
