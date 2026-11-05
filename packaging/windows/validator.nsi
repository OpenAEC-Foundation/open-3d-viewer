; Per-user installer of Open BIM Validator; no administrator rights needed.
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

Name "Open BIM Validator ${NUMBER}"
!ifndef OUTPUT
  !define OUTPUT "open-bim-validator_${NUMBER}_x64-setup.exe"
!endif
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\Open BIM Validator"
RequestExecutionLevel user
SetCompressor /SOLID lzma
!define MUI_ICON "icon.ico"
!define MUI_UNICON "icon.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\open-bim-validator.exe"
!define UNINST "Software\Microsoft\Windows\CurrentVersion\Uninstall\Open BIM Validator"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Dutch"
!insertmacro MUI_LANGUAGE "English"

VIProductVersion "${NUMBER}.0"
VIAddVersionKey /LANG=${LANG_ENGLISH} "ProductName" "Open BIM Validator"
VIAddVersionKey /LANG=${LANG_ENGLISH} "CompanyName" "OpenAEC Foundation"
VIAddVersionKey /LANG=${LANG_ENGLISH} "FileVersion" "${NUMBER}"
VIAddVersionKey /LANG=${LANG_ENGLISH} "ProductVersion" "${NUMBER}"
VIAddVersionKey /LANG=${LANG_ENGLISH} "FileDescription" "Open BIM Validator installer"
VIAddVersionKey /LANG=${LANG_ENGLISH} "LegalCopyright" "OpenAEC Foundation"

Section "Install"
  SetOutPath "$INSTDIR"
  File "${SOURCE}\open-bim-validator.exe"
  File "icon.ico"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortcut "$SMPROGRAMS\Open BIM Validator.lnk" "$INSTDIR\open-bim-validator.exe" "" "$INSTDIR\icon.ico"
  WriteRegStr HKCU "${UNINST}" "DisplayName" "Open BIM Validator"
  WriteRegStr HKCU "${UNINST}" "DisplayVersion" "${NUMBER}"
  WriteRegStr HKCU "${UNINST}" "Publisher" "OpenAEC Foundation"
  WriteRegStr HKCU "${UNINST}" "DisplayIcon" "$INSTDIR\icon.ico"
  WriteRegStr HKCU "${UNINST}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKCU "${UNINST}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\Open BIM Validator.lnk"
  Delete "$INSTDIR\open-bim-validator.exe"
  Delete "$INSTDIR\icon.ico"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${UNINST}"
SectionEnd
