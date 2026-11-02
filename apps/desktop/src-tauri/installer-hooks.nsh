; Knock's installer hooks (SPEC.md §9.7), included by Tauri's NSIS template
; (tauri.conf.json bundle.windows.nsis.installerHooks).
;
; The finish page's "Create desktop shortcut" box starts unticked (user
; decision, 2026-11-02). Tauri has no setting for it; MUI2 reads this define
; in the template's MUI_PAGE_FINISH, which comes after this file. Silent
; installs (/S) still create one unless /NS is passed (the template's own
; rule); the updater's /UPDATE never does.
;
; Uninstall is Tauri's default: it deletes Run\Knock and leaves the Task
; Manager mark (ADR 0028).

!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED
