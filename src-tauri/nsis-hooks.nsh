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

  ; BUG 1/2: the same class of locked-file failure applies to every OTHER
  ; supervisor/untracked partner binary FEM ships resources for or installs
  ; alongside itself. The running app's own release_install_tree/orphan-sweep
  ; cover the normal update path, but a manual installer run (or an update
  ; from a version that predates those sweeps) can still hit a locked file
  ; here. Best-effort, same "non-zero = nothing matched" contract.
  nsExec::Exec 'taskkill /F /T /IM titan-edge.exe'
  Pop $1
  nsExec::Exec 'taskkill /F /T /IM sdk_client.exe'
  Pop $1
  nsExec::Exec 'taskkill /F /T /IM space-acres.exe'
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

!macro NSIS_HOOK_POSTINSTALL
  ; BUG 1/2: verify the main binary actually landed and is not a truncated/
  ; missing copy. The observed failure mode is Windows Defender quarantining
  ; the freshly-extracted file mid-install ("file contains a virus or
  ; potentially unwanted software") — the installer reported success while
  ; the app itself was gone or broken. A 1 MB floor is well below the real
  ; binary's size and well above an empty/stub file.
  ${If} ${FileExists} "$INSTDIR\fry-edge-miner.exe"
    FileOpen $3 "$INSTDIR\fry-edge-miner.exe" r
    FileSeek $3 0 END $4
    FileClose $3
  ${Else}
    StrCpy $4 0
  ${EndIf}

  IntOp $5 1024 * 1024

  ${If} $4 < $5
    DetailPrint "fry-edge-miner.exe missing or truncated after install (size: $4 bytes) — likely quarantined by antivirus"
    FileOpen $6 "$INSTDIR\update-failed.txt" w
    FileWrite $6 "Fry Edge Miner failed to install correctly.$\r$\n"
    FileWrite $6 "The most likely cause is antivirus/Defender quarantining the new file during install.$\r$\n"
    FileWrite $6 "$\r$\n"
    FileWrite $6 "To recover:$\r$\n"
    FileWrite $6 "  1. Open Windows Security > Virus & threat protection > Protection history, find the quarantined fry-edge-miner.exe, and restore it.$\r$\n"
    FileWrite $6 "  2. Or restore the previous version from: $APPDATA\com.frynetworks.fem\.prev\fry-edge-miner.exe$\r$\n"
    FileWrite $6 "  3. Add an exclusion for this folder in Windows Security so future updates are unaffected: $INSTDIR$\r$\n"
    FileClose $6
    MessageBox MB_ICONEXCLAMATION "Fry Edge Miner did not install correctly — the application file is missing or was blocked by antivirus.$\r$\n$\r$\nSee $INSTDIR\update-failed.txt for recovery steps, or restore the previous version from $APPDATA\com.frynetworks.fem\.prev\"
  ${Else}
    DetailPrint "fry-edge-miner.exe verified present ($4 bytes)"
  ${EndIf}

  ; Defender exclusion + frynode firewall rule are deliberately NOT attempted
  ; here. Measured on a real dev box: `Add-MpPreference` alone took ~19s to
  ; return (Defender PowerShell module load), even though it is GUARANTEED
  ; to fail anyway — this installer runs UNELEVATED by design (installMode
  ; "currentUser", see the WebView2 comment above), and Add-MpPreference
  ; requires admin. Two sequential calls would add ~30-40s of dead time to
  ; EVERY install for a command that cannot succeed in this context. The
  ; real mechanism is the app's own one-time ELEVATED setup
  ; (security_setup.rs), which runs both together under a single UAC prompt
  ; on first launch and before each update, and actually succeeds.
!macroend
