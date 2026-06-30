# dual-pane-text-rendering

## Purpose

定义 Dual 双模数据显示面板中 ASCII 文本和 HEX 十六进制的渲染规则，确保控制字符的可视化表现与文本终端行为一致。

## Requirements

### Requirement: Dual 模式 ASCII 文本渲染
系统必须在 Dual 模式的左侧 ASCII 面板中正确渲染控制字符，使其与文本终端的行为可视化对应。

#### Scenario: 回车符 \r 渲染
- **WHEN** Dual 模式帧数据中包含 `\r` (0x0D) 字符
- **THEN** 左侧 ASCII 面板必须将该字符渲染为 Unicode 控制图像符号 `␍` (U+240D)，而非丢弃或隐藏

#### Scenario: 换行符 \n 渲染
- **WHEN** Dual 模式帧数据中包含 `\n` (0x0A) 字符
- **THEN** 左侧 ASCII 面板必须将该字符渲染为 Unicode 控制图像符号 `␊` (U+240A)，而非丢弃或隐藏

#### Scenario: 普通可打印字符渲染
- **WHEN** Dual 模式帧数据中包含可打印 ASCII 字符（0x20–0x7E）
- **THEN** 左侧 ASCII 面板必须原样显示该字符

#### Scenario: 其他控制字符渲染
- **WHEN** Dual 模式帧数据中包含非回车/换行的控制字符（0x00–0x1F 排除 0x0D/0x0A）或高位字节（0x7F–0xFF）
- **THEN** 左侧 ASCII 面板必须将其渲染为 `.` 占位符

#### Scenario: 右侧 HEX 面板字节间距
- **WHEN** Dual 模式帧数据转换为 HEX 字符串
- **THEN** 每两个十六进制字符（一个字节）之间必须有一个空格分隔（如 `48 65 6C 6C 6F`），每 8 字节后额外增加一个空格表示分组边界
