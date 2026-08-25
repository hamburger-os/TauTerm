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

  ; ── 注册并启动 TauTerm 特权服务（虚拟串口后端，LocalSystem） ──
  DetailPrint "TauTerm: Registering TauTermService..."
  ; 若服务已存在（升级中断/残留注册），其 binPath 可能指向旧版本或已删除的
  ; 二进制。先 stop + delete 重建，确保指向当前安装目录。
  ExecWait 'sc.exe query TauTermService' $2
  ${If} $2 == 0
    ExecWait 'sc.exe stop TauTermService' $2
    ExecWait 'sc.exe delete TauTermService' $2
  ${EndIf}
  ExecWait 'sc.exe create TauTermService binPath= "$INSTDIR\tauterm-service.exe" start= delayed-auto' $2
  ${If} $2 != 0
    MessageBox MB_ICONEXCLAMATION \
      "Failed to register TauTermService (exit code $2).$\n\
       Virtual serial port feature will not be available."
  ${Else}
    ExecWait 'sc.exe failure TauTermService reset= 86400 actions= restart/5000/restart/5000/restart/5000' $2
    ExecWait 'sc.exe start TauTermService' $2
    ${If} $2 != 0
      MessageBox MB_ICONEXCLAMATION \
        "Failed to start TauTermService (exit code $2).$\n\
         Virtual serial port feature will not be available."
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; ── 停止并删除特权服务 ──
  ExecWait 'sc.exe stop TauTermService' $0
  ExecWait 'sc.exe delete TauTermService' $0

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
