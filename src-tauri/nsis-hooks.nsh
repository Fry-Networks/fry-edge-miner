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

!macro NSIS_HOOK_PREINSTALL
  !if "${WEBVIEW2BOOTSTRAPPERPATH}" != ""
    ; Extract embedded bootstrapper with unique temp name
    ; (avoids collision with Section WebView2's MicrosoftEdgeWebview2Setup.exe)
    File "/oname=$TEMP\WebView2Repair.exe" "${WEBVIEW2BOOTSTRAPPERPATH}"

    DetailPrint "Verifying WebView2 Runtime for current user..."
    ExecWait '"$TEMP\WebView2Repair.exe" /silent /install' $1

    ${If} $1 = 0
      DetailPrint "WebView2 Runtime OK."
    ${Else}
      ; Non-zero exit is acceptable — bootstrapper returns non-zero
      ; for "already installed system-level" but the repair side-effect
      ; (fixing per-user loader state) still occurs
      DetailPrint "WebView2 repair attempted (exit: $1). Continuing."
    ${EndIf}

    Delete "$TEMP\WebView2Repair.exe"
  !endif
!macroend
