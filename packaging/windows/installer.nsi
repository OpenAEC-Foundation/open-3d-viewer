; Per-user installer of Open 3D Viewer Studio; no administrator rights needed.
; makensis /DNUMBER=2026.44.0 /DSOURCE=<folder with open-3d-viewer.exe and open-3d-viewer-mcp.exe> [/DSIGN=<windows-sign.ps1>] installer.nsi
Unicode true
!include "MUI2.nsh"

; With SIGN, makensis hands the uninstaller it builds to the signing script before it goes into
; the installer, so the uninstall.exe that WriteUninstaller puts on disk is signed as well. A
; failed signature stops the build.
!ifdef SIGN
  !if ${NSIS_PACKEDVERSION} < 0x03008000
    !error "signing the uninstaller needs NSIS 3.08 or later (!uninstfinalize)"
  !endif
  !uninstfinalize 'pwsh -NoProfile -NonInteractive -File "${SIGN}" -Files "%1"' = 0
!endif

Name "Open 3D Viewer Studio ${NUMBER}"
OutFile "open-3d-viewer_${NUMBER}_x64-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\Open 3D Viewer Studio"
RequestExecutionLevel user
SetCompressor /SOLID lzma
!define MUI_ICON "icon.ico"
!define MUI_UNICON "icon.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\open-3d-viewer.exe"
!define UNINST "Software\Microsoft\Windows\CurrentVersion\Uninstall\Open 3D Viewer Studio"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Dutch"
!insertmacro MUI_LANGUAGE "English"

VIProductVersion "${NUMBER}.0"
VIAddVersionKey /LANG=${LANG_ENGLISH} "ProductName" "Open 3D Viewer Studio"
VIAddVersionKey /LANG=${LANG_ENGLISH} "CompanyName" "OpenAEC Foundation"
VIAddVersionKey /LANG=${LANG_ENGLISH} "FileVersion" "${NUMBER}"
VIAddVersionKey /LANG=${LANG_ENGLISH} "ProductVersion" "${NUMBER}"
VIAddVersionKey /LANG=${LANG_ENGLISH} "FileDescription" "Open 3D Viewer Studio installer"
VIAddVersionKey /LANG=${LANG_ENGLISH} "LegalCopyright" "OpenAEC Foundation"

Section "Install"
  SetOutPath "$INSTDIR"
  File "${SOURCE}\open-3d-viewer.exe"
  File "${SOURCE}\open-3d-viewer-mcp.exe"
  File "icon.ico"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortcut "$SMPROGRAMS\Open 3D Viewer Studio.lnk" "$INSTDIR\open-3d-viewer.exe" "" "$INSTDIR\icon.ico"
  WriteRegStr HKCU "${UNINST}" "DisplayName" "Open 3D Viewer Studio"
  WriteRegStr HKCU "${UNINST}" "DisplayVersion" "${NUMBER}"
  WriteRegStr HKCU "${UNINST}" "Publisher" "OpenAEC Foundation"
  WriteRegStr HKCU "${UNINST}" "DisplayIcon" "$INSTDIR\icon.ico"
  WriteRegStr HKCU "${UNINST}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKCU "${UNINST}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\Open 3D Viewer Studio.lnk"
  Delete "$INSTDIR\open-3d-viewer.exe"
  Delete "$INSTDIR\open-3d-viewer-mcp.exe"
  Delete "$INSTDIR\icon.ico"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${UNINST}"
SectionEnd
