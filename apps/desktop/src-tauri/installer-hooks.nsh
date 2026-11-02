; Knock's uninstall hooks (SPEC.md §9.7, ADR 0025), included by Tauri's NSIS
; template (tauri.conf.json bundle.windows.nsis.installerHooks).
;
; Tauri's uninstaller deletes HKCU\...\Run\Knock whatever it starts. ADR 0025
; removes it only if it starts Knock's app. So, unless updating:
; 1. Knock is closed first; saying no cancels with nothing changed.
; 2. `knock-desktop.exe --remove-sign-in` applies the ADR 0025 rule: the value
;    and its StartupApproved mark go only if the value starts Knock's app.
; 3. A value still there starts another program: a copy (type and bytes as
;    they are) is written back after Tauri's delete.
; If the app can't be run, no copy is kept, so Tauri's delete applies.
; Never touches %APPDATA%\Knock or any Claude Code file.

!define /ifndef KNOCK_RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
; HKEY_CURRENT_USER, as the 32-bit uninstaller passes it.
!define KNOCK_HKCU 0x80000001
; RRF_RT_ANY | RRF_NOEXPAND: any type, REG_EXPAND_SZ as written.
!define KNOCK_RRF 0x1000FFFF

Var KnockRunType
Var KnockRunSize
Var KnockRunData

; Copies Run\Knock into the variables above, if it exists.
!macro KnockKeepRunValue
  System::Call 'advapi32::RegGetValueW(p ${KNOCK_HKCU}, w "${KNOCK_RUN_KEY}", w "Knock", i ${KNOCK_RRF}, *i .r1, p 0, *i 0 r2) i .r0'
  ${If} $0 = 0
    System::Alloc $2
    Pop $3
    System::Call 'advapi32::RegGetValueW(p ${KNOCK_HKCU}, w "${KNOCK_RUN_KEY}", w "Knock", i ${KNOCK_RRF}, *i .r1, p r3, *i r2 r2) i .r0'
    ${If} $0 = 0
      StrCpy $KnockRunType $1
      StrCpy $KnockRunSize $2
      StrCpy $KnockRunData $3
    ${Else}
      System::Free $3
    ${EndIf}
  ${EndIf}
!macroend

; Writes the copy back, if there is one.
!macro KnockRestoreRunValue
  ${If} $KnockRunData P<> 0
    System::Call 'advapi32::RegSetKeyValueW(p ${KNOCK_HKCU}, w "${KNOCK_RUN_KEY}", w "Knock", i $KnockRunType, p $KnockRunData, i $KnockRunSize) i .r0'
    System::Free $KnockRunData
    StrCpy $KnockRunData 0
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  StrCpy $KnockRunData 0
  ${If} $UpdateMode <> 1
    !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
    ClearErrors
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --remove-sign-in' $0
    ${IfNot} ${Errors}
    ${AndIf} $0 = 0
      !insertmacro KnockKeepRunValue
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro KnockRestoreRunValue
!macroend
