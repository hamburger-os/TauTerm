; TauTerm NSIS Installer Hooks
; Installs the com0com kernel driver during setup and removes it on uninstall.
; Requires Tauri v2 bundle.windows.nsis.installerHooks configuration.
;
; With installMode: "perMachine" in tauri.conf.json, the NSIS installer
; uses RequestExecutionLevel highestAvailable. If the user accepts the UAC
; prompt, this driver installation will succeed.
;
; com0com v3 没有独立的"仅安装驱动"命令。驱动通过创建第一个端口对
; 自动安装。此 hook 使用默认端口名 (CNCA0/CNCB0) 创建临时端口对
; 触发驱动安装，然后立即删除，只保留驱动程序。
; CNCA0/CNCB0 是驱动内部名称，用户不会在设备管理器中看到 COM99 编号。

!include "LogicLib.nsh"

!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "TauTerm: Installing com0com virtual serial port driver..."

  ${If} ${FileExists} "$INSTDIR\setupc.exe"
  ${AndIf} ${FileExists} "$INSTDIR\com0com.sys"
    SetOutPath "$INSTDIR"

    ; 在总线 0 上创建临时端口对以触发驱动安装
    ExecWait '"$INSTDIR\setupc.exe" install 0 - -' $0

    ${If} $0 == 0
      DetailPrint "TauTerm: com0com driver installed."
      ; 删除临时端口对（用总线号 0 而非端口名 CNCA0），只保留驱动程序
      ExecWait '"$INSTDIR\setupc.exe" remove 0' $1
    ${ElseIf} $0 == 1
      DetailPrint "TauTerm: com0com driver already installed."
    ${Else}
      MessageBox MB_ICONEXCLAMATION \
        "com0com driver installation returned code $0.$\n\
         Virtual serial port feature may not work."
    ${EndIf}
  ${Else}
    MessageBox MB_ICONEXCLAMATION \
      "com0com driver files not found in $INSTDIR.$\n\
       Virtual serial port feature will not be available."
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} ${FileExists} "$INSTDIR\setupc.exe"
    DetailPrint "TauTerm: Removing com0com virtual serial port driver..."
    SetOutPath "$INSTDIR"
    ExecWait '"$INSTDIR\setupc.exe" uninstall' $0
    ${If} $0 == 0
      DetailPrint "TauTerm: com0com driver removed."
    ${Else}
      DetailPrint "TauTerm: com0com driver removal completed with code $0."
    ${EndIf}
  ${EndIf}
!macroend
