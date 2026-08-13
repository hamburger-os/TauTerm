# riperf3 vendor fork

- **基线**: riperf3 0.8.0（crates.io，Evan Henry，MIT OR Apache-2.0）
- **来源**: `C:\Users\hnthinker\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\riperf3-0.8.0\`（原样拷贝，已删除 Cargo.lock）
- **引用方式**: `src-tauri/Cargo.toml` → `riperf3 = { path = "vendor/riperf3" }`（包名不变，导入零改动）

## 补丁清单（相对上游 0.8.0）

1. **逐秒区间通道**（TauTerm iperf3 实时输出的核心补丁）：
   - `src/reporter.rs` — 新增 `IntervalSender` 新类型（std mpsc `Sender` 无 `PartialEq`，参照 `InterruptWatch` #210 惯例实现 `PartialEq`/`UnwindSafe`/`RefUnwindSafe`）；`IntervalReporterConfig` 增加 `interval_tx: Option<IntervalSender>`；区间构造点（`Interval { streams, sum, sum_bidir_reverse }` 构建后）在通道存在时 `tx.0.send(interval.clone())`，与 `json_stream` 标志独立。
   - `src/reporter.rs` — `collecting` 门条件改为 `collector.is_some() || config.interval_tx.is_some()`：无 -J/--json-stream 的宿主配置（TauTerm）不挂 collector，通道仍需每 tick 构造类型化 `Interval`。
   - `src/client.rs` / `src/server.rs` — `ClientBuilder`/`ServerBuilder` 增加 `pub fn interval_channel(tx: std::sync::mpsc::Sender<Interval>) -> Self`，字段穿入 `Client`/`Server` 结构体并转发到 reporter 配置；`want_collector` 增加 `|| self.interval_tx.is_some()`，保证最终 `Report.intervals` 与通道流出的区间一致（宿主可在结束时核对）。
   - `tests/interval_channel.rs` — 新增实时性回归测试（客户端 TCP/UDP、服务端：首个区间必须在 run 结束前到达，通道数量与 Report 一致）。

2. **quiet 静音模式**（TauTerm console 干净 + 文件日志保留）：
   - `src/macros.rs` — `OUTPUT_QUIET`/`output_quiet()`/`OutputQuietGuard`（保存/恢复前值，并发 run 正确嵌套）；`vprintln!` 宏 quiet 时跳过 `println!`，保留 `log::info!` 与 capture。
   - `src/reporter.rs` — `titled()` 打印点 quiet 门控（capture 保留）。
   - `src/server.rs` — `banner_line` 与错误 `eprintln!` 门控；`src/client.rs` — SERVER ERROR `eprintln!`、-J blob、get-server-output 打印门控。
   - `ClientBuilder`/`ServerBuilder` 新增 `pub fn quiet(bool) -> Self`（Default false），字段穿入 `Client`/`Server`；`Client::run()`/`Server::run()`/`Server::run_once()` 入口挂 guard。
   - `tests/interval_channel.rs` — `quiet_mode_keeps_live_channel` 冒烟测试（静音不误伤逐秒通道）。

3. **Windows dead_code 警告修复**：
   - `src/stream.rs` — `set_final_retransmits`/`set_final_tcp_sample` 加 `#[cfg(any(unix, test))]`（全部调用点在 `#[cfg(unix)]` TCP_INFO 路径或测试内；getter 无条件保留）。

4. **bind 前置 / 监听器复用**（TauTerm 服务端状态准确性）：
   - `src/server.rs` — `run_once()` 拆分为 `bind()` + `run_once_with_listener(&TcpListener)`：宿主先 bind（端口占用等绑定失败在 emit `running:true` 之前暴露，无"先绿后红"闪烁），再复用同一监听器循环 `run_once_with_listener`（消除每轮重新 bind 的窗口）。`run_once()` 保持组合语义（`run()` 不受影响）。

## 上游同步步骤

1. 从 crates.io 下载新版本源码，覆盖本目录（保留本文件）。
2. 按上述补丁清单逐条重放。
3. `cargo check` 验证；跑互通矩阵（官方 iperf3.exe）确认行为不变。

## 验证矩阵（2026-08 实测通过）

| 组合 | 结果 |
|---|---|
| fork 客户端（带通道）→ 官方 iperf3 3.21 服务端 | ✅ `official_iperf3_client_interop`（16.9 Gbps，逐秒实时断言） |
| 官方 iperf3 3.21 客户端 → fork 服务端（带通道） | ✅ `official_iperf3_server_interop`（28.2 Gbps，逐秒实时断言，shell 驱动） |
| fork ↔ fork 逐秒通道（TCP/UDP/服务端） | ✅ `interval_channel` 3 个测试 |
| fork 全套测试（integration 等） | ✅ 全过 |

**已知测试环境怪癖（非库缺陷）**：Windows 上 Cygwin 版 iperf3 客户端若由
**承载 fork 服务端的同一进程** spawn，会在 cookie 发送前挂起；从独立 shell
（真实用户方式）或服务端放独立进程时完全正常。因此服务端互通测试为
shell 驱动（`iperf3.exe -c 127.0.0.1 -p 5213 -t 3 -i 1`）。

## 可选后续

- 把 `interval_channel` 特性向上游提 PR；合入后删除本目录、恢复 `riperf3 = "<新版本>"`。
