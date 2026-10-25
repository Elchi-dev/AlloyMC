; Runs after Alloy's files are copied: fetch the block textures from Mojang now,
; so the first launch already has them. Failure is fine; the app retries on start.
!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "Fetching block textures from Mojang..."
  nsExec::ExecToLog '"$INSTDIR\${MAINBINARYNAME}.exe" --fetch-assets'
!macroend
