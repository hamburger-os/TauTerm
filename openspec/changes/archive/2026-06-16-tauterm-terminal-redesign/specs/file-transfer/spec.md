# File Transfer (Delta)

## MODIFIED Requirements

### Requirement: 文件传输界面
系统必须在界面中提供专用的文件传输面板，用于发起和监控传输。面板必须支持拖拽上传（Dropzone）。

#### Scenario: 打开文件传输面板
- **WHEN** 用户点击状态栏中的文件传输按钮或按下 Ctrl+Shift+F
- **THEN** 必须从底部滑出一个玻璃面板，包含"发送文件"、"接收文件"按钮和传输历史列表

#### Scenario: 拖拽文件上传
- **WHEN** 用户从桌面拖拽一个文件进入 TauTerm 窗口
- **THEN** 整个窗口变暗（rgba(0,0,0,0.4) 遮罩），文件传输面板自动滑出并产生呼吸闪烁的青色边框，面板中央显示 "⚡ Drop to Transfer" 提示文字

#### Scenario: 放置文件开始传输
- **WHEN** 用户在 Dropzone 中松开放置的文件
- **THEN** 面板播放一道扫光动画（Scan-line Sweep），随后自动启动 YModem 发送

#### Scenario: 拖拽离开取消
- **WHEN** 用户将文件拖出 TauTerm 窗口
- **THEN** 遮罩消失，面板恢复正常状态，不启动传输

### Requirement: 传输进度显示
进度条必须包含流光扫光动画效果。

#### Scenario: 传输进度动画
- **WHEN** YModem 文件传输进行中
- **THEN** 进度条必须包含从左向右的流光扫光动画，配合百分比数字显示

## ADDED Requirements

### Requirement: Dropzone 视觉反馈
文件传输面板必须作为 Dropzone 工作，提供完整的拖拽视觉反馈。

#### Scenario: 拖拽进入窗口
- **WHEN** 文件被拖入 TauTerm 窗口
- **THEN** 系统检测到拖拽事件，整个窗口覆盖半透明暗色遮罩

#### Scenario: 拖拽悬停在传输面板上
- **WHEN** 拖拽的文件悬停在文件传输面板上方
- **THEN** 面板边框以青色高频呼吸闪烁（1s 周期），背景略微变亮
