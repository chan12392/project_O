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
!macroend
