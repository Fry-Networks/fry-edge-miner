; Fry Edge Miner — NSIS installer hooks
; Fixes broken per-user WebView2 states where system-level install exists
; in registry but Tauri runtime can't create a WebView2 environment.
;
; Execution order in generated installer.nsi:
;   Section WebView2  → checks registry, skips if pv key found
;   Section Install   → THIS HOOK → file copy → registry → shortcuts
;
; The bootstrapper is idempotent:
;   - Missing WebView2    → installs it (exit 0)
;   - Broken per-user     → repairs loader state (exit 0 or non-zero)
;   - Healthy WebView2    → exits quickly, no-op (exit 0)

; The installer is built installMode:"currentUser" so a standard (non-admin)
; account can install without elevation. Running the Evergreen bootstrapper
; unconditionally broke that promise: on the Windows 11 default (WebView2 already
; present machine-wide) it touches HKLM/Program Files, so Windows raises an
; elevation/credential prompt a standard user cannot satisfy. The installer then
; ignored the outcome and continued — the "install error you must ignore to
; continue" field report. Probe the registry first and only repair when there is
; no healthy runtime to use.

!macro _WV2_CHECK OUTVAR
  ; A non-empty `pv` under the Evergreen runtime client GUID means installed.
  StrCpy ${OUTVAR} ""
  ReadRegStr ${OUTVAR} HKLM \
    "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  ${If} ${OUTVAR} == ""
    ReadRegStr ${OUTVAR} HKLM \
      "SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  ${EndIf}
  ${If} ${OUTVAR} == ""
    ReadRegStr ${OUTVAR} HKCU \
      "SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  ; B7 (installer side): partner binaries are launched from the install tree
  ; this installer is about to overwrite. The running app stops them before an
  ; auto-update, but a manual installer run — or an update from a version that
  ; predates that stop — leaves frynode.exe holding
  ; resources\frynode.exe open, and the file copy fails with
  ; "Error opening file for writing". Kill it here; a non-zero exit just means
  ; nothing matched, which is the normal case.
  DetailPrint "Stopping partner processes that hold the install folder..."
  nsExec::Exec 'taskkill /F /T /IM frynode.exe'
  Pop $1
  Sleep 2000

  !if "${WEBVIEW2BOOTSTRAPPERPATH}" != ""
    !insertmacro _WV2_CHECK $2

    ${If} $2 != ""
    ${AndIf} $2 != "0.0.0.0"
      ; A healthy runtime is already registered — do NOT run the bootstrapper.
      ; This is the common case and the one that used to trigger elevation.
      DetailPrint "WebView2 Runtime already present (version $2). Skipping repair."
    ${Else}
      ; Extract embedded bootstrapper with unique temp name
      ; (avoids collision with Section WebView2's MicrosoftEdgeWebview2Setup.exe)
      File "/oname=$TEMP\WebView2Repair.exe" "${WEBVIEW2BOOTSTRAPPERPATH}"

      DetailPrint "Installing WebView2 Runtime for current user..."
      ExecWait '"$TEMP\WebView2Repair.exe" /silent /install' $1

      ${If} $1 = 0
        DetailPrint "WebView2 Runtime OK."
      ${Else}
        DetailPrint "WebView2 install attempted (exit: $1). Continuing."
      ${EndIf}

      Delete "$TEMP\WebView2Repair.exe"
    ${EndIf}
  !endif
!macroend
