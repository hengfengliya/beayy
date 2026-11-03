; Beayy 的 Windows 安装包（NSIS）。脚本/发布安装包.sh 调：
;   makensis -DVERSION=0.2.43 -DEXE=…/beayy.exe -DICON=…/icon.ico -DOUT=…/Beayy-0.2.43-windows-x64-setup.exe windows/installer.nsi
; 只装给当前用户（%LOCALAPPDATA%\Programs\Beayy，不要管理员）：自动更新能直接换 beayy.exe。
; 建开始菜单和桌面快捷方式、打开开机自动启动（app 设置里能关）、在「应用和功能」里能卸载。数据在 %APPDATA%\Beayy，卸载不删。
Unicode true
ManifestDPIAware true
RequestExecutionLevel user
SetCompressor /SOLID lzma

!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Beayy"

Name "Beayy"
OutFile "${OUT}"
InstallDir "$LOCALAPPDATA\Programs\Beayy"
BrandingText "Be Authentic. Your Yesterdays."

!include MUI2.nsh
!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_FINISHPAGE_RUN "$INSTDIR\beayy.exe"
!define MUI_FINISHPAGE_RUN_PARAMETERS "--open"
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"

Section
  ; 在跑的先关掉：同一个位置的换不了文件；以前解压 zip 跑的也关，免得两个一起采
  nsExec::Exec 'taskkill /IM beayy.exe /F'
  Sleep 1000
  SetOutPath "$INSTDIR"
  File "/oname=beayy.exe" "${EXE}"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortcut "$SMPROGRAMS\Beayy.lnk" "$INSTDIR\beayy.exe" "--open"
  CreateShortcut "$DESKTOP\Beayy.lnk" "$INSTDIR\beayy.exe" "--open"
  WriteRegStr HKCU "${RUN_KEY}" "Beayy" '"$INSTDIR\beayy.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "Beayy"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\beayy.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "Beayy"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "https://beayy.com"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  nsExec::Exec 'taskkill /IM beayy.exe /F'
  Sleep 1000
  Delete "$INSTDIR\beayy.exe"
  Delete "$INSTDIR\beayy.old.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Beayy.lnk"
  Delete "$DESKTOP\Beayy.lnk"
  DeleteRegValue HKCU "${RUN_KEY}" "Beayy"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
