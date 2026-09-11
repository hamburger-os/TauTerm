; TauTerm NSIS Installer Hooks
; Installs the com0com kernel driver during setup. On uninstall, the shared
; system driver is removed only when TauTerm installed it and no port pairs
; remain; third-party com0com installations and endpoints are never removed.
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

; ── 私有工具宏 ──────────────────────────────────────────
;
; 递归删除目录；若因短时文件锁失败则等待重试，确保 Geek 等工具
; 在卸载器退出后立刻扫描时目录已被真正删除，而非被 /REBOOTOK
; 标记成"重启后删除"而残留空目录。
; 注意：RMDir /r 对正在运行的 uninstall.exe 会失败并重试，重试耗尽后
; 由 NSIS 卸载器的"临时副本自删除"机制在退出前补删。
!macro RMDIR_Retry _dir _maxsec
  ; 用 LogicLib 的 Do/LoopWhile 而非 Goto+固定标签，确保宏被多次展开时
  ; 不会重复定义标签导致编译失败。
  ; 注意：`_dir` 传入时已带引号，先存入寄存器 $R8；直接把它塞进
  ; ${FileExists} 会因 $COMMONAPPDATA 之类的变量被当成常量而告警。
  StrCpy $R9 0
  StrCpy $R8 ${_dir}
  ${Do}
    ClearErrors
    RMDir /r "$R8"
    ; RMDir 成功后目录已不存在，结束；否则说明仍有锁，等待后重试
    ${IfNot} ${FileExists} "$R8"
      ${Break}
    ${EndIf}
    Sleep 1000
    IntOp $R9 $R9 + 1
    ${LoopWhile} $R9 < ${_maxsec}
!macroend

; Tauri updater 以 /UPDATE 启动同一个 NSIS 安装器，并在更新模式下原地覆盖
; 文件，而不会先运行旧版本卸载器。因此必须在 Tauri 模板复制新文件之前
; 停掉 TauTermService；否则正在运行的 tauterm-service.exe 可能锁住旧文件，
; 导致在线更新无法覆盖。该 hook 位于 Tauri 的 CheckIfAppIsRunning 和 File
; 指令之前。普通手工安装/重装仍交给 Tauri 默认流程处理，不在这里强杀进程。
!macro NSIS_HOOK_PREINSTALL
  ${If} $UpdateMode = 1
    DetailPrint "TauTerm: Preparing running components for online update..."

    ; updater 通过 ShellExecute 启动安装器后才退出当前 GUI 进程。这里再次
    ; 结束主进程和 TauTerm 专属 WebView2 子进程，消除启动/退出之间的竞态。
    ExecWait '"taskkill.exe" /IM tauterm.exe /F /T' $0
    ExecWait '"taskkill.exe" /F /IM msedgewebview2.exe /FI "COMMANDLINE eq *TauTerm*" /T' $0

    ; 服务是 LocalSystem 常驻进程，必须在复制 tauterm-service.exe 前停止。
    ; 此处只停止进程以释放文件锁，不提前删除 SCM 注册：删除动作留到
    ; POSTINSTALL，避免 service marked-for-deletion 状态横跨文件复制阶段。
    ExecWait 'sc.exe stop TauTermService' $0
    StrCpy $R7 0
    ${Do}
      ExecWait '"taskkill.exe" /F /IM tauterm-service.exe /T' $0
      Sleep 300
      IntOp $R7 $R7 + 1
    ${LoopWhile} $R7 < 5
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "TauTerm: Installing com0com virtual serial port driver..."

  ${If} ${FileExists} "$INSTDIR\setupc.exe"
  ${AndIf} ${FileExists} "$INSTDIR\com0com.sys"
    SetOutPath "$INSTDIR"

    ; com0com 是系统级共享驱动。先记录安装前是否已经存在，只有从“未安装”
    ; 状态由 TauTerm 成功装入时才写 ownership marker。这样卸载 TauTerm 不会
    ; 把用户/其它软件原本已有的 com0com 驱动当成自己的资源。
    ExecWait 'sc.exe query com0com' $R6

    ; 在总线 0 上创建临时端口对以触发驱动安装
    ExecWait '"$INSTDIR\setupc.exe" install 0 - -' $0

    ${If} $0 == 0
      DetailPrint "TauTerm: com0com driver installed."
      ${If} $R6 <> 0
        CreateDirectory "$COMMONAPPDATA\TauTerm\service"
        FileOpen $R5 "$COMMONAPPDATA\TauTerm\service\driver-owned.marker" w
        FileWrite $R5 "installed-by-tauterm$\r$\n"
        FileClose $R5
      ${EndIf}

      ; 删除临时端口对（用总线号 0 而非端口名 CNCA0），只保留驱动程序；
      ; 若删除失败会在设备管理器遗留一个可见的 COM 端口对，需提示用户。
      ExecWait '"$INSTDIR\setupc.exe" remove 0' $1
      ${If} $1 <> 0
        MessageBox MB_ICONEXCLAMATION \
          "com0com 临时端口对移除失败 (code $1).$\n\
           设备管理器中可能遗留 CNCA0/CNCB0 端口对，但不影响使用。"
      ${EndIf}
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

    ; sc delete 可能短暂进入 MARKED_FOR_DELETE 状态；在 create 之前等待 SCM
    ; 真正确认旧服务消失，否则紧接着 sc create 会偶发 1072。
    StrCpy $R7 0
    ${Do}
      ExecWait 'sc.exe query TauTermService' $2
      ${If} $2 <> 0
        ${Break}
      ${EndIf}
      Sleep 250
      IntOp $R7 $R7 + 1
    ${LoopWhile} $R7 < 20
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
  ; 关键：卸载器进程的 CWD 绝不能停留在 $INSTDIR。SetOutPath "$INSTDIR" 会
  ; 让当前目录指向安装目录，Windows 拒绝删除"正被当作工作目录"的目录，
  ; 从而产生空目录残留（此前用 /REBOOTOK + 异步 cmd 补偿的根因）。
  ; 这里先把 CWD 移到临时目录释放该锁。
  SetOutPath "$TEMP"

  ; ── 结束仍在运行的 TauTerm 程序 / WebView2 进程 ──
  ; 先结束 GUI，让服务端观察到客户端管道关闭并有机会按 ownership 释放本次
  ; 会话创建的端口；不要在这里同时强杀服务，否则会打断正常资源回收。
  DetailPrint "TauTerm: Terminating running application processes..."
  ExecWait '"taskkill.exe" /IM tauterm.exe /F /T' $0
  ExecWait '"taskkill.exe" /F /IM msedgewebview2.exe /FI "COMMANDLINE eq *TauTerm*" /T' $0
  Sleep 1000

  ; ── 停止并删除特权服务（LocalSystem，binPath 指向安装目录）──
  ; 先请求优雅停止，让服务完成已知 ownership 的回收；随后才强杀作为兜底，
  ; 避免卸载时人为制造 orphan。最后删除 SCM 注册并释放二进制文件锁。
  ExecWait 'sc.exe stop TauTermService' $0
  Sleep 1500
  StrCpy $R7 0
  ${Do}
    ExecWait '"taskkill.exe" /F /IM tauterm-service.exe /T' $0
    Sleep 300
    IntOp $R7 $R7 + 1
  ${LoopWhile} $R7 < 3
  ExecWait 'sc.exe delete TauTermService' $0

  ; ── 安全处理 com0com 系统级共享驱动 ──
  ; 只有“TauTerm 最初安装了驱动”且驱动中已经没有任何端口对时才卸载驱动。
  ; 若仍有任意端口对，它可能属于第三方，也可能是未能安全回收的资源；此时
  ; 宁可保留共享驱动，也绝不通过全局 uninstall 删除无法证明属于 TauTerm 的资源。
  ${If} ${FileExists} "$INSTDIR\setupc.exe"
    ${If} ${FileExists} "$COMMONAPPDATA\TauTerm\service\driver-owned.marker"
      SetOutPath "$INSTDIR"
      ExecWait '"$SYSDIR\cmd.exe" /D /S /C ""$INSTDIR\setupc.exe" list | findstr.exe /R /C:"CNCA[0-9]" /C:"CNCB[0-9]" >nul 2>&1"' $R6
      ${If} $R6 == 1
        DetailPrint "TauTerm: Removing TauTerm-owned com0com driver..."
        ExecWait '"$INSTDIR\setupc.exe" uninstall' $0
        ${If} $0 <> 0
          Sleep 500
          ExecWait '"$INSTDIR\setupc.exe" uninstall' $0
        ${EndIf}
        DetailPrint "TauTerm: com0com driver removal completed with code $0."
      ${ElseIf} $R6 == 0
        DetailPrint "TauTerm: com0com port pairs still exist; shared driver left installed for safety."
      ${Else}
        DetailPrint "TauTerm: unable to verify com0com port ownership; shared driver left installed for safety."
      ${EndIf}
      SetOutPath "$TEMP"
    ${Else}
      DetailPrint "TauTerm: com0com was not installed by TauTerm; shared driver left untouched."
    ${EndIf}
  ${EndIf}

  ; ── 清理特权服务的机器级 ownership 状态 ──
  ; 在线升级必须保留该状态，供新服务恢复异常中断资源；只有真正卸载时删除。
  DetailPrint "TauTerm: Removing privileged virtual-port state..."
  !insertmacro RMDIR_Retry "$COMMONAPPDATA\TauTerm\service" 3
  ; 若 TauTerm 下没有其它机器级数据，顺带删除空父目录；非空时 RMDir 会安全失败。
  RMDir "$COMMONAPPDATA\TauTerm"

  ; ── 同步清理安装目录 ──
  ; 标准卸载流程（Uninstall 段）会在最后 RMDir "$INSTDIR"，并由 NSIS 的
  ; "临时副本自删除"机制在退出前补删 uninstall.exe 与目录。此处结束所有进程
  ; 且 CWD 已移出 $INSTDIR，再做一次同步清理（不带 /REBOOTOK、不派生子进程），
  ; 保证 Geek 等工具在卸载器退出后立刻扫描时目录已被删除。
  DetailPrint "TauTerm: Removing install directory ($INSTDIR)..."
  !insertmacro RMDIR_Retry "$INSTDIR" 5
!macroend
