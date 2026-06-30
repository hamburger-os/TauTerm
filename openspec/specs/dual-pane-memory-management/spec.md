# dual-pane-memory-management

## Purpose

定义终端行缓冲的内存管理策略，统一应用于所有数据模式（Text / HEX / Dual），防止长时间会话导致行缓冲无限增长和内存耗尽。

## Requirements

### Requirement: 统一终端行缓冲限制
系统必须对所有数据模式的终端显示缓冲进行统一的行数上限限制，防止长时间会话导致内存耗尽。行缓冲上限由 `tauterm-buffer-lines` 设置项控制，默认 10,000 行，范围 1,000–100,000，通过 AppearanceSettings 滑块配置。xterm.js (Text/HEX) 通过 `scrollback` 选项应用，DualPane 在 `flushDualLines` 的 setState updater 中裁剪。

#### Scenario: 达到行数上限时裁剪旧数据
- **WHEN** 终端行缓冲中的行数达到配置的上限且有新数据到达
- **THEN** 系统必须移除最早的行（FIFO 策略），将总行数保持在上限以内
- **AND** 此行为在 Text（xterm scrollback）、HEX（xterm scrollback）和 Dual（DualPane 行缓冲）三种模式下一致

#### Scenario: 会话断开时清理缓冲
- **WHEN** 任何模式的会话断开
- **THEN** 系统必须清除该会话对应的所有缓冲区数据（xterm 实例 + DualPane 行缓冲 + 分帧缓冲区 + RAF pending 队列）

#### Scenario: 行数上限实时配置
- **WHEN** 用户通过设置滑块修改行缓冲上限
- **THEN** 新上限必须立即生效：xterm 通过 `terminal.options.scrollback = N` 动态更新，DualPane 在下一次 RAF flush 时使用新上限裁剪
