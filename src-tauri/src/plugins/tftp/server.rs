//! TFTP 服务端适配层
//!
//! 手动实现 listen loop，复用 `tftpd::Worker`、`tftpd::Packet`、
//! `tftpd::OptionsProtocol` 等所有协议类型。
//!
//! 适配层的 listen loop 完全控制请求处理流程：
//!   接收 UDP 包 → 解析 Packet → 创建 Worker 传输

use std::net::{SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tftpd::{ErrorCode, Packet, Socket as TftpSocket};

/// 服务端最大并发传输数
const MAX_CONCURRENT_SERVER_TRANSFERS: u64 = 32;

use super::counting_socket::CountingSocket;
use super::transfer;
use super::{TftpConfig, TftpDynamicParams};

/// TFTP 服务端线程运行上下文。
pub struct TftpServerContext {
    pub app: AppHandle,
    pub socket: Arc<UdpSocket>,
    pub config: TftpConfig,
    pub params: Arc<Mutex<TftpDynamicParams>>,
    pub abort: Arc<AtomicBool>,
    pub server_running: Arc<AtomicBool>,
    pub next_transfer_id: Arc<AtomicU64>,
    pub active_server_transfers: Arc<AtomicU64>,
    pub session_id: String,
}

#[derive(Clone)]
struct ServerTransferContext {
    app: AppHandle,
    session_id: String,
    root: PathBuf,
    params: Arc<Mutex<TftpDynamicParams>>,
    abort: Arc<AtomicBool>,
    next_transfer_id: Arc<AtomicU64>,
}

struct ServerProgressMeta {
    app: AppHandle,
    session_id: String,
    transfer_id: String,
    filename: String,
    direction: String,
}

/// 启动 TFTP 服务端 listen 循环。
///
/// 在独立的 `std::thread` 中运行阻塞 UDP 循环。
/// 通过 `abort` 标志实现优雅停止。
/// `server_running` 在 listen loop 启动/停止时自动设置。
pub fn spawn_tftp_server(context: TftpServerContext) {
    let TftpServerContext {
        app,
        socket,
        config,
        params,
        abort,
        server_running,
        next_transfer_id,
        active_server_transfers,
        session_id,
    } = context;
    let root = PathBuf::from(&config.file_root);
    let write_enabled = config.write_enabled;
    let overwrite = config.overwrite;
    let transfer_context = ServerTransferContext {
        app: app.clone(),
        session_id: session_id.clone(),
        root: root.clone(),
        params: params.clone(),
        abort: abort.clone(),
        next_transfer_id: next_transfer_id.clone(),
    };

    // 设置超时以便定期检查 abort flag
    if let Err(e) = socket.set_read_timeout(Some(Duration::from_secs(1))) {
        log::error!("[TFTP Server] 设置读超时失败: {}", e);
        server_running.store(false, std::sync::atomic::Ordering::Relaxed);
        let _ = app.emit(
            "tftp-server-status",
            serde_json::json!({
                "session_id": session_id,
                "running": false,
                "error": format!("设置读超时失败: {}", e),
            }),
        );
        return;
    }

    std::thread::spawn(move || {
        // 启动前检查 abort 标志，防止 shutdown() 在 spawn 返回后线程尚未开始执行时误判
        if abort.load(Ordering::Relaxed) {
            return;
        }
        server_running.store(true, std::sync::atomic::Ordering::Relaxed);
        // 权威状态发布：socket 在连接时已绑定成功，此处线程真实进入监听。
        // tftp_server_start 不再无条件乐观 emit——Start/Stop 交错导致线程在
        // 启动前 abort 检查处退出时，乐观的 running:true 会永久失真
        //（keepAlive 会话 getStatus 只查一次，无事件可纠正）
        let _ = app.emit(
            "tftp-server-status",
            serde_json::json!({
                "session_id": session_id,
                "running": true,
                "listen_addr": socket.local_addr().map(|a| a.to_string()).ok(),
            }),
        );
        log::info!("[TFTP Server] 开始监听 (session={})", session_id);

        // 复用缓冲区，避免每次循环分配 64 KB
        let mut buf = vec![0u8; 65536];

        loop {
            if abort.load(Ordering::Relaxed) {
                log::info!("[TFTP Server] 收到停止信号 (session={})", session_id);
                break;
            }

            let (len, from) = match socket.recv_from(&mut buf) {
                Ok(result) => result,
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    continue;
                }
                Err(e) => {
                    log::error!("[TFTP Server] recv_from 错误: {}", e);
                    break;
                }
            };

            let packet = match Packet::deserialize(&buf[..len]) {
                Ok(p) => p,
                Err(e) => {
                    log::warn!("[TFTP Server] 无效数据包 (from {}): {}", from, e);
                    continue;
                }
            };

            match packet {
                Packet::Rrq {
                    ref filename,
                    ref mode,
                    ref options,
                } => {
                    log::info!("[TFTP Server] RRQ from {}: {} ({})", from, filename, mode);
                    // 并发限制检查
                    if active_server_transfers.load(Ordering::Relaxed)
                        >= MAX_CONCURRENT_SERVER_TRANSFERS
                    {
                        log::warn!(
                            "[TFTP Server] 并发传输数已达上限 ({})，拒绝 RRQ from {}",
                            MAX_CONCURRENT_SERVER_TRANSFERS,
                            from
                        );
                        let _ = TftpSocket::send_to(
                            socket.as_ref(),
                            &Packet::Error {
                                code: ErrorCode::AccessViolation,
                                msg: "server busy, too many concurrent transfers".to_string(),
                            },
                            &from,
                        );
                        continue;
                    }
                    // 在独立线程处理传输，不阻塞 listen loop
                    let transfer_context = transfer_context.clone();
                    let active_xfer = active_server_transfers.clone();
                    let filename = filename.clone();
                    let options = options.to_vec();
                    active_server_transfers.fetch_add(1, Ordering::Relaxed);
                    std::thread::spawn(move || {
                        handle_rrq(&transfer_context, from, &filename, &options);
                        active_xfer.fetch_sub(1, Ordering::Relaxed);
                    });
                }
                Packet::Wrq {
                    ref filename,
                    ref mode,
                    ref options,
                } => {
                    log::info!("[TFTP Server] WRQ from {}: {} ({})", from, filename, mode);
                    if !write_enabled {
                        let _ = TftpSocket::send_to(
                            socket.as_ref(),
                            &Packet::Error {
                                code: ErrorCode::AccessViolation,
                                msg: "server is read-only".to_string(),
                            },
                            &from,
                        );
                        continue;
                    }
                    // 并发限制检查
                    if active_server_transfers.load(Ordering::Relaxed)
                        >= MAX_CONCURRENT_SERVER_TRANSFERS
                    {
                        log::warn!(
                            "[TFTP Server] 并发传输数已达上限 ({})，拒绝 WRQ from {}",
                            MAX_CONCURRENT_SERVER_TRANSFERS,
                            from
                        );
                        let _ = TftpSocket::send_to(
                            socket.as_ref(),
                            &Packet::Error {
                                code: ErrorCode::AccessViolation,
                                msg: "server busy, too many concurrent transfers".to_string(),
                            },
                            &from,
                        );
                        continue;
                    }
                    // 在独立线程处理传输，不阻塞 listen loop
                    let transfer_context = transfer_context.clone();
                    let active_xfer = active_server_transfers.clone();
                    let filename = filename.clone();
                    let options = options.to_vec();
                    active_server_transfers.fetch_add(1, Ordering::Relaxed);
                    std::thread::spawn(move || {
                        handle_wrq(&transfer_context, from, &filename, &options, overwrite);
                        active_xfer.fetch_sub(1, Ordering::Relaxed);
                    });
                }
                _ => {
                    log::debug!("[TFTP Server] 未处理包 from {}: {:?}", from, packet);
                }
            }
        }

        server_running.store(false, std::sync::atomic::Ordering::Relaxed);
        // 不在此 emit running:false：退出仅由 abort 触发，断开/停止路径已
        // 各自权威 emit；重连后旧线程（≤1s 收尾窗口）的迟到 emit 反而会
        // 翻转新服务器的状态
        log::info!("[TFTP Server] 已停止 (session={})", session_id);
    });
}

fn handle_rrq(
    context: &ServerTransferContext,
    remote: SocketAddr,
    filename: &str,
    options: &[tftpd::TransferOption],
) {
    let ServerTransferContext {
        app,
        session_id,
        root,
        params: params_lock,
        abort,
        next_transfer_id,
    } = context;
    let transfer_id = next_transfer_id.fetch_add(1, Ordering::Relaxed).to_string();

    let file_path = sanitize_filename(filename);
    let full_path = root.join(&file_path);

    // 创建一个临时 socket 用于发送错误响应
    let send_error = |code: ErrorCode, msg: &str| {
        if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
            let _ = TftpSocket::send_to(
                &sock,
                &Packet::Error {
                    code,
                    msg: msg.to_string(),
                },
                &remote,
            );
        }
    };

    if !validate_file_path(&full_path, root) {
        log::warn!("[TFTP Server] 路径遍历攻击: {}", filename);
        send_error(ErrorCode::AccessViolation, "access violation");
        return;
    }
    if !full_path.exists() {
        log::warn!("[TFTP Server] 文件不存在: {}", full_path.display());
        send_error(ErrorCode::FileNotFound, "file not found");
        return;
    }
    if full_path.is_dir() {
        log::warn!("[TFTP Server] 请求的是目录: {}", full_path.display());
        send_error(ErrorCode::AccessViolation, "cannot read a directory");
        return;
    }

    let file_size = full_path.metadata().map(|m| m.len()).unwrap_or(0);
    let params = params_lock.lock().unwrap().clone();

    let transfer_socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(e) => {
            log::error!("[TFTP Server] 创建传输 socket 失败: {}", e);
            return;
        }
    };
    if let Err(e) = transfer_socket.connect(remote) {
        log::warn!(
            "[TFTP Server] transfer socket connect({}) 失败: {} (将继续使用 send_to)",
            remote,
            e
        );
    }

    // 设置读超时，使用协商后的 timeout_secs（而非硬编码 3s）
    let oack_wait = Duration::from_secs(params.timeout_secs.max(1) as u64);
    if let Err(e) = transfer_socket.set_read_timeout(Some(oack_wait)) {
        log::warn!("[TFTP Server] 设置传输 socket 读超时失败: {}", e);
    }

    // 发送 OACK（如果有选项）或直接开始
    let has_options = !options.is_empty();
    if has_options {
        let oack = super::build_oack_options(&params, Some(file_size));
        if tftpd::Socket::send_to(&transfer_socket, &Packet::Oack(oack), &remote).is_err() {
            return;
        }

        // 检查 abort 标志后再阻塞等待 ACK
        if abort.load(Ordering::Relaxed) {
            log::info!("[TFTP Server] RRQ 在 OACK 等待期间被中止");
            return;
        }

        match tftpd::Socket::recv_from(&transfer_socket) {
            Ok((Packet::Ack(0), _)) => {}
            Ok((Packet::Error { code, msg }, _)) => {
                log::warn!("[TFTP Server] RRQ OACK 响应错误: {} - {}", code, msg);
                return;
            }
            Err(e) => {
                log::warn!("[TFTP Server] RRQ OACK recv 错误: {}", e);
                return;
            }
            _ => {
                log::warn!("[TFTP Server] RRQ OACK 后收到意外包");
                return;
            }
        }
    }

    let mut counting = setup_counting_socket(
        transfer_socket,
        remote,
        Some(file_size),
        params.blksize,
        ServerProgressMeta {
            app: app.clone(),
            session_id: session_id.clone(),
            transfer_id: transfer_id.clone(),
            filename: filename.to_string(),
            direction: "upload".into(),
        },
    );

    match transfer::send_file(&mut counting, full_path, remote, &params, abort) {
        Ok(result) => {
            let cksum = result.checksum.map(|c| format!("{:08X}", c));
            let avg_bps = (result.bytes_transferred * 1000)
                .checked_div(result.duration_ms)
                .unwrap_or(0);
            log::info!(
                "[TFTP Server] RRQ 完成 [{}]: {} ({} bytes, CRC32={})",
                transfer_id,
                filename,
                result.bytes_transferred,
                cksum.as_deref().unwrap_or("N/A")
            );
            let _ = app.emit(
                "tftp-transfer-done",
                serde_json::json!({
                    "session_id": session_id,
                    "transfer_id": transfer_id,
                    "filename": filename,
                    "success": true,
                    "bytes": result.bytes_transferred,
                    "checksum": cksum,
                    "avg_bytes_per_second": avg_bps,
                    "is_server": true,
                }),
            );
        }
        Err(e) => {
            log::error!("[TFTP Server] RRQ 失败 [{}]: {}", transfer_id, e);
            let _ = app.emit(
                "tftp-transfer-done",
                serde_json::json!({
                    "session_id": session_id,
                    "transfer_id": transfer_id,
                    "filename": filename,
                    "success": false,
                    "error": e,
                    "is_server": true,
                }),
            );
        }
    }
}

fn handle_wrq(
    context: &ServerTransferContext,
    remote: SocketAddr,
    filename: &str,
    options: &[tftpd::TransferOption],
    overwrite: bool,
) {
    let ServerTransferContext {
        app,
        session_id,
        root,
        params: params_lock,
        abort,
        next_transfer_id,
    } = context;
    let transfer_id = next_transfer_id.fetch_add(1, Ordering::Relaxed).to_string();

    let file_path = sanitize_filename(filename);
    let full_path = root.join(&file_path);

    let send_error = |code: ErrorCode, msg: &str| {
        if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
            let _ = TftpSocket::send_to(
                &sock,
                &Packet::Error {
                    code,
                    msg: msg.to_string(),
                },
                &remote,
            );
        }
    };

    if !validate_file_path(&full_path, root) {
        log::warn!("[TFTP Server] 路径遍历攻击: {}", filename);
        send_error(ErrorCode::AccessViolation, "access violation");
        return;
    }
    if full_path.exists() && !overwrite {
        log::warn!(
            "[TFTP Server] 文件已存在（不允许覆盖）: {}",
            full_path.display()
        );
        send_error(ErrorCode::FileExists, "file already exists");
        return;
    }

    let params = params_lock.lock().unwrap().clone();

    let transfer_socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(e) => {
            log::error!("[TFTP Server] 创建传输 socket 失败: {}", e);
            return;
        }
    };
    if let Err(e) = transfer_socket.connect(remote) {
        log::warn!(
            "[TFTP Server] transfer socket connect({}) 失败: {} (将继续使用 send_to)",
            remote,
            e
        );
    }

    // 设置读超时，使用协商后的 timeout_secs（对齐 handle_rrq）
    let oack_wait = Duration::from_secs(params.timeout_secs.max(1) as u64);
    if let Err(e) = transfer_socket.set_read_timeout(Some(oack_wait)) {
        log::warn!("[TFTP Server] 设置传输 socket 读超时失败: {}", e);
    }

    let has_options = !options.is_empty();
    if has_options {
        let oack = super::build_oack_options(&params, None);
        let _ = tftpd::Socket::send_to(&transfer_socket, &Packet::Oack(oack), &remote);
    } else {
        let _ = tftpd::Socket::send_to(&transfer_socket, &Packet::Ack(0), &remote);
    }

    // 从 WRQ options 中提取 TransferSize（客户端 PUT 时告知文件大小）
    let tsize = options
        .iter()
        .find(|o| o.option == tftpd::OptionType::TransferSize)
        .map(|o| o.value);

    let mut counting = setup_counting_socket(
        transfer_socket,
        remote,
        tsize,
        params.blksize,
        ServerProgressMeta {
            app: app.clone(),
            session_id: session_id.clone(),
            transfer_id: transfer_id.clone(),
            filename: filename.to_string(),
            direction: "download".into(),
        },
    );

    match transfer::receive_file(&mut counting, full_path, remote, &params, abort, 1) {
        Ok(result) => {
            let cksum = result.checksum.map(|c| format!("{:08X}", c));
            let avg_bps = (result.bytes_transferred * 1000)
                .checked_div(result.duration_ms)
                .unwrap_or(0);
            log::info!(
                "[TFTP Server] WRQ 完成 [{}]: {} ({} bytes, CRC32={})",
                transfer_id,
                filename,
                result.bytes_transferred,
                cksum.as_deref().unwrap_or("N/A")
            );
            let _ = app.emit(
                "tftp-transfer-done",
                serde_json::json!({
                    "session_id": session_id,
                    "transfer_id": transfer_id,
                    "filename": filename,
                    "success": true,
                    "bytes": result.bytes_transferred,
                    "checksum": cksum,
                    "avg_bytes_per_second": avg_bps,
                    "is_server": true,
                }),
            );
        }
        Err(e) => {
            log::error!("[TFTP Server] WRQ 失败 [{}]: {}", transfer_id, e);
            // 清理不完整文件
            if params.clean_on_error {
                let file_path = root.join(sanitize_filename(filename));
                let _ = std::fs::remove_file(&file_path);
            }
            let _ = app.emit(
                "tftp-transfer-done",
                serde_json::json!({
                    "session_id": session_id,
                    "transfer_id": transfer_id,
                    "filename": filename,
                    "success": false,
                    "error": e,
                    "is_server": true,
                }),
            );
        }
    }
}

/// 共享辅助函数：将已连接并完成协商的 UDP socket 包装为 CountingSocket，
/// 并设置进度回调（包含 remote_addr）。
///
/// `socket` 应当已经 `bind` 并 `connect` 到对端。
/// `total_size` 传入文件大小以支持进度百分比（WRQ 传 `None`）。
fn setup_counting_socket(
    socket: UdpSocket,
    remote: SocketAddr,
    total_size: Option<u64>,
    blksize: u16,
    meta: ServerProgressMeta,
) -> CountingSocket {
    let ServerProgressMeta {
        app: app_clone,
        session_id: sid,
        transfer_id: xfer_id,
        filename: fname,
        direction: dir,
    } = meta;
    let remote_str = remote.to_string();

    let counting = CountingSocket::new(socket, remote, blksize as usize);
    if let Some(size) = total_size {
        counting.set_total_size(size);
    }
    counting.set_progress_callback(Box::new(move |p| {
        let _ = app_clone.emit(
            "tftp-transfer-progress",
            serde_json::json!({
                "session_id": sid,
                "transfer_id": xfer_id,
                "filename": fname,
                "bytes_transferred": p.bytes_transferred,
                "total_bytes": p.total_bytes,
                "blocks_transferred": p.blocks_transferred,
                "bytes_per_second": p.bytes_per_second,
                "is_server": true,
                "remote_addr": remote_str,
                "direction": dir,
            }),
        );
    }));

    counting
}

/// 清理文件名：去除 Windows 盘符（如 "C:\"）、前导斜杠、.. 等危险字符
fn sanitize_filename(filename: &str) -> PathBuf {
    // 跳过 Windows 盘符 (如 "C:file.txt" 或 "C:\file.txt")
    let bytes = filename.as_bytes();
    let without_drive = if bytes.len() >= 2 && bytes[1] == b':' {
        &filename[2..]
    } else {
        filename
    };
    // 去除前导斜杠
    let trimmed = without_drive.trim_start_matches(['/', '\\']);
    PathBuf::from(trimmed)
}

fn validate_file_path(path: &Path, root: &Path) -> bool {
    if path.to_string_lossy().contains("..") {
        return false;
    }
    if let Some(parent) = path.parent() {
        path.starts_with(root) || parent.starts_with(root)
    } else {
        path.starts_with(root)
    }
}
