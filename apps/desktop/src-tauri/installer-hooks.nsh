; Knock's installer hooks (SPEC.md §9.7, ADR 0025), included by Tauri's NSIS
; template (tauri.conf.json bundle.windows.nsis.installerHooks).
;
; Install: the finish page's "Create desktop shortcut" box starts unticked
; (user decision, 2026-11-02). Tauri has no setting for it; MUI2 reads this
; define in the template's MUI_PAGE_FINISH, which comes after this file.
; Silent installs (/S) still create one unless /NS is passed (the template's
; own rule); the updater's /UPDATE never does.
;
; Uninstall: Tauri's uninstaller deletes HKCU\...\Run\Knock whatever it
; starts. ADR 0025 removes it only if it starts Knock's app. So, unless
; updating:
; 1. Knock is closed first; saying no cancels with nothing changed.
; 2. A copy of Run\Knock (type and bytes as they are) is taken.
; 3. `knock-desktop.exe --remove-sign-in` applies the ADR 0025 rule: the value
;    and its StartupApproved mark go only if the value starts Knock's app.
; 4. If it ran and exited 0, the copy is replaced by whatever value is left
;    (another program's), or dropped if none is. If it couldn't run or
;    failed, the copy is kept: a value not proven Knock's is never deleted.
; 5. After Tauri's delete, any copy kept is written back.
; Never touches %APPDATA%\Knock or any Claude Code file.

!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED

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

; Drops the copy, if there is one.
!macro KnockForgetRunValue
  ${If} $KnockRunData P<> 0
    System::Free $KnockRunData
    StrCpy $KnockRunData 0
  ${EndIf}
!macroend

; Writes the copy back, if there is one.
!macro KnockRestoreRunValue
  ${If} $KnockRunData P<> 0
    System::Call 'advapi32::RegSetKeyValueW(p ${KNOCK_HKCU}, w "${KNOCK_RUN_KEY}", w "Knock", i $KnockRunType, p $KnockRunData, i $KnockRunSize) i .r0'
  ${EndIf}
  !insertmacro KnockForgetRunValue
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  StrCpy $KnockRunData 0
  ${If} $UpdateMode <> 1
    !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
    !insertmacro KnockKeepRunValue
    ClearErrors
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --remove-sign-in' $0
    ${IfNot} ${Errors}
    ${AndIf} $0 = 0
      !insertmacro KnockForgetRunValue
      !insertmacro KnockKeepRunValue
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro KnockRestoreRunValue
!macroend
