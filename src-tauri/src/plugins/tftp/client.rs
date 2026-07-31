//! TFTP 客户端操作
//!
//! 使用自研 transfer 引擎（而非 tftpd::Client，因其内部类型未公开）。
//! GET/PUT 在 `tokio::task::spawn_blocking` 中执行同步 UDP I/O。

use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tftpd::{Packet, Socket, TransferOption};

use super::counting_socket::CountingSocket;
use super::transfer;
use super::TftpDynamicParams;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// 辅助：为 CountingSocket 设置客户端进度回调
fn setup_client_progress(
    counting: &CountingSocket,
    app: &AppHandle,
    session_id: &str,
    transfer_id: &str,
    filename: &str,
    remote_addr: &str,
    direction: &str,
) {
    let app = app.clone();
    let sid = session_id.to_string();
    let xid = transfer_id.to_string();
    let fname = filename.to_string();
    let raddr = remote_addr.to_string();
    let dir = direction.to_string();

    counting.set_progress_callback(Box::new(move |p| {
        let _ = app.emit("tftp-transfer-progress", serde_json::json!({
            "session_id": sid,
            "transfer_id": xid,
            "filename": fname,
            "bytes_transferred": p.bytes_transferred,
            "total_bytes": p.total_bytes,
            "blocks_transferred": p.blocks_transferred,
            "bytes_per_second": p.bytes_per_second,
            "is_server": false,
            "direction": dir,
            "remote_addr": raddr,
        }));
    }));
}

/// TFTP 客户端 GET：从远程服务器下载文件
///
/// 协议流程：
/// 1. 发送 RRQ → 接收 OACK 或 DATA[1]
/// 2. OACK 路径：发送 ACK[0] → 接收 DATA[1]
/// 3. 写入首块 → 发送 ACK[1] → 通过 `transfer::receive_file` 完成剩余块
pub async fn tftp_client_get(
    app: AppHandle,
    session_id: String,
    transfer_id: String,
    remote_ip: String,
    remote_port: u16,
    remote_filename: String,
    local_path: PathBuf,
    params: TftpDynamicParams,
) -> Result<(), String> {
    let remote: SocketAddr = format!("{}:{}", remote_ip, remote_port).parse()
        .map_err(|e| format!("无效地址: {}", e))?;

    log::info!("[TFTP Client GET] 开始: transfer_id={}, session={}, remote={}, file={}, dest={}",
        transfer_id, session_id, remote, remote_filename, local_path.display());

    let fname = remote_filename.clone();
    let sid = session_id.clone();
    let xid = transfer_id.clone();
    // 克隆供 spawn_blocking 闭包内部使用（闭包会 move 所有权）
    let app_inner = app.clone();
    let fname_inner = fname.clone();

    let handle = tokio::task::spawn_blocking(move || {
        let xfer_started = std::time::Instant::now();

        // ── 阶段 1：绑定 socket 并发送 RRQ ──
        let socket = UdpSocket::bind("0.0.0.0:0")
            .map_err(|e| format!("绑定失败: {}", e))?;
        socket.set_read_timeout(Some(REQUEST_TIMEOUT))
            .map_err(|e| format!("设置超时失败: {}", e))?;

        let rrq = Packet::Rrq {
            filename: fname_inner.clone(),
            mode: "octet".into(),
            options: vec![
                TransferOption { option: tftpd::OptionType::BlockSize, value: params.blksize as u64 },
                TransferOption { option: tftpd::OptionType::Timeout, value: params.timeout_secs as u64 },
            ],
        };
        tftpd::Socket::send_to(&socket, &rrq, &remote)
            .map_err(|e| format!("RRQ 发送失败: {}", e))?;

        // ── 阶段 2：接收初始响应（OACK 或 DATA[1]）──
        // 使用 recv_from_with_size 而非 recv_from，因为 recv_from 默认缓冲区仅 512 字节，
        // blksize > 512 时服务器直接发送 DATA[1] 会导致 Windows WSAEMSGSIZE (10040)。
        let (transfer_addr, file_size, first_data, negotiated_blksize): (SocketAddr, u64, Vec<u8>, u16) = match tftpd::Socket::recv_from_with_size(&socket, params.blksize as usize) {
            Ok((Packet::Oack(options), from)) => {
                let blksize = options.iter()
                    .find(|o| o.option == tftpd::OptionType::BlockSize)
                    .map(|o| o.value as u16)
                    .unwrap_or(params.blksize);
                let tsize = options.iter()
                    .find(|o| o.option == tftpd::OptionType::TransferSize)
                    .map(|o| o.value)
                    .unwrap_or(0);
                log::info!("[TFTP Client GET] OACK tsize={}, blksize={}", tsize, blksize);
                socket.connect(from).map_err(|e| format!("connect 失败: {}", e))?;
                // 发送 ACK[0] 确认 OACK
                tftpd::Socket::send(&socket, &Packet::Ack(0))
                    .map_err(|e| format!("ACK(0) 发送失败: {}", e))?;
                // 接收 DATA[1] — 使用协商后的 blksize 作为缓冲区大小
                match tftpd::Socket::recv_with_size(&socket, blksize as usize) {
                    Ok(Packet::Data { block_num: 1, data }) => (from, tsize, data, blksize),
                    Ok(Packet::Error { code, msg }) => {
                        return Err(format!("服务器错误: {} - {}", code, msg));
                    }
                    other => {
                        return Err(format!("OACK 后期望 DATA[1]，收到: {:?}", other));
                    }
                }
            }
            Ok((Packet::Data { block_num: 1, data }, from)) => {
                socket.connect(from).map_err(|e| format!("connect 失败: {}", e))?;
                (from, 0, data, params.blksize)
            }
            Ok((Packet::Error { code, msg }, _)) => {
                return Err(format!("服务器错误: {} - {}", code, msg));
            }
            other => {
                return Err(format!("期望 OACK 或 DATA[1]，收到: {:?}", other));
            }
        };

        // ── 阶段 3：写入首块，创建 CountingSocket ──
        let first_block_len = first_data.len() as u64;
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&local_path)
                .map_err(|e| format!("创建文件失败: {}", e))?;
            file.write_all(&first_data).map_err(|e| format!("写入首块失败: {}", e))?;
        }

        // CRC32: 计算首块哈希
        let mut full_hasher = crc32fast::Hasher::new();
        full_hasher.update(&first_data);

        let abort = Arc::new(AtomicBool::new(false));
        let mut counting = CountingSocket::new(socket, transfer_addr, negotiated_blksize as usize);
        if file_size > 0 {
            counting.set_total_size(file_size);
        }
        counting.record_data_bytes(first_block_len);

        setup_client_progress(
            &counting, &app_inner, &sid, &xid, &fname_inner,
            &transfer_addr.to_string(), "download",
        );

        // 发送 ACK[1] 确认首块
        counting.send_to(&Packet::Ack(1), &transfer_addr)
            .map_err(|e| format!("ACK(1) 发送失败: {}", e))?;

        // ── 阶段 4：通过 transfer 引擎接收剩余块（start_block=2）──
        match transfer::receive_file(&mut counting, local_path.clone(), transfer_addr, &params, &abort, 2) {
            Ok(result) => {
                // 合并 CRC32：首块 + receive_file 处理的所有后续块
                let total_bytes = first_block_len + result.bytes_transferred;
                // 重新读取完整文件以计算 CRC32（简单可靠的方式）
                let checksum = match std::fs::read(&local_path) {
                    Ok(all_data) => {
                        let mut h = crc32fast::Hasher::new();
                        h.update(&all_data);
                        Some(h.finalize())
                    }
                    Err(_) => result.checksum, // 回退：仅包含 receive_file 部分的校验和
                };
                let cksum_str = checksum.map(|c| format!("{:08X}", c));
                let duration_ms = xfer_started.elapsed().as_millis() as u64;
                let avg_bps = if duration_ms > 0 { (total_bytes * 1000) / duration_ms } else { 0 };
                Ok((total_bytes, cksum_str, avg_bps, duration_ms))
            }
            Err(e) => Err(e),
        }
    });

    match handle.await {
        Ok(Ok((bytes, checksum, avg_bps, _duration_ms))) => {
            let _ = app.emit("tftp-transfer-done", serde_json::json!({
                "session_id": session_id,
                "transfer_id": transfer_id,
                "filename": fname,
                "success": true,
                "bytes": bytes,
                "checksum": checksum,
                "avg_bytes_per_second": avg_bps,
                "is_server": false,
            }));
            log::info!("[TFTP] GET 完成 [{}]: {} ({} bytes, CRC32={})", transfer_id, fname, bytes, checksum.as_deref().unwrap_or("N/A"));
            Ok(())
        }
        Ok(Err(e)) => {
            let _ = app.emit("tftp-transfer-done", serde_json::json!({
                "session_id": session_id,
                "transfer_id": transfer_id,
                "filename": fname,
                "success": false,
                "error": e,
                "is_server": false,
            }));
            Err(e)
        }
        Err(e) => {
            let msg = format!("TFTP GET panic: {}", e);
            let _ = app.emit("tftp-transfer-done", serde_json::json!({
                "session_id": session_id,
                "transfer_id": transfer_id,
                "filename": fname,
                "success": false,
                "error": msg,
                "is_server": false,
            }));
            Err(msg)
        }
    }
}

/// TFTP 客户端 PUT：上传文件到远程服务器
///
/// 协议流程：
/// 1. 发送 WRQ → 接收 ACK[0] 或 OACK
/// 2. 通过 `transfer::send_file` 完成数据传输
pub async fn tftp_client_put(
    app: AppHandle,
    session_id: String,
    transfer_id: String,
    remote_ip: String,
    remote_port: u16,
    remote_filename: String,
    local_path: PathBuf,
    params: TftpDynamicParams,
) -> Result<(), String> {
    let remote: SocketAddr = format!("{}:{}", remote_ip, remote_port).parse()
        .map_err(|e| format!("无效地址: {}", e))?;

    log::info!("[TFTP Client PUT] 开始: transfer_id={}, session={}, remote={}, file={}, src={}",
        transfer_id, session_id, remote, remote_filename, local_path.display());

    let fname = remote_filename.clone();
    let file_size = std::fs::metadata(&local_path)
        .map(|m| m.len()).unwrap_or(0);

    let sid = session_id.clone();
    let xid = transfer_id.clone();
    let app_inner = app.clone();
    let fname_inner = fname.clone();

    let handle = tokio::task::spawn_blocking(move || {
        let socket = UdpSocket::bind("0.0.0.0:0")
            .map_err(|e| format!("绑定失败: {}", e))?;
        socket.set_read_timeout(Some(REQUEST_TIMEOUT))
            .map_err(|e| format!("设置超时失败: {}", e))?;

        // 发送 WRQ
        let wrq = Packet::Wrq {
            filename: remote_filename,
            mode: "octet".into(),
            options: vec![
                TransferOption { option: tftpd::OptionType::BlockSize, value: params.blksize as u64 },
                TransferOption { option: tftpd::OptionType::TransferSize, value: file_size },
                TransferOption { option: tftpd::OptionType::Timeout, value: params.timeout_secs as u64 },
            ],
        };
        tftpd::Socket::send_to(&socket, &wrq, &remote)
            .map_err(|e| format!("WRQ 发送失败: {}", e))?;

        // 等待 ACK[0] 或 OACK —— 记录服务端 transfer socket 的实际地址
        // 同时提取协商后的 blksize（OACK 路径），确保 CountingSocket 和
        // send_file 使用的块大小与服务端一致
        let (transfer_addr, negotiated_blksize) = match tftpd::Socket::recv_from(&socket) {
            Ok((Packet::Ack(0), from)) => {
                socket.connect(from).map_err(|e| format!("connect 失败: {}", e))?;
                (from, params.blksize)
            }
            Ok((Packet::Oack(options), from)) => {
                let blksize = options.iter()
                    .find(|o| o.option == tftpd::OptionType::BlockSize)
                    .map(|o| o.value as u16)
                    .unwrap_or(params.blksize);
                socket.connect(from).map_err(|e| format!("connect 失败: {}", e))?;
                (from, blksize)
            }
            Ok((Packet::Error { code, msg }, _)) => {
                return Err(format!("服务器错误: {} - {}", code, msg));
            }
            other => {
                return Err(format!("期望 ACK(0) 或 OACK，收到: {:?}", other));
            }
        };

        // 用 transfer 引擎发送
        let abort = Arc::new(AtomicBool::new(false));
        // 使用协商后的 blksize 创建 CountingSocket
        let mut counting = CountingSocket::new(socket, transfer_addr, negotiated_blksize as usize);
        counting.set_total_size(file_size);

        setup_client_progress(
            &counting, &app_inner, &sid, &xid, &fname_inner,
            &transfer_addr.to_string(), "upload",
        );

        transfer::send_file(&mut counting, local_path, transfer_addr, &params, &abort)
    });

    match handle.await {
        Ok(Ok(result)) => {
            let cksum = result.checksum.map(|c| format!("{:08X}", c));
            let avg_bps = if result.duration_ms > 0 {
                (result.bytes_transferred * 1000) / result.duration_ms
            } else { 0 };
            log::info!("[TFTP] PUT 完成 [{}]: {} ({} bytes, CRC32={})",
                transfer_id, fname, result.bytes_transferred,
                cksum.as_deref().unwrap_or("N/A"));
            let _ = app.emit("tftp-transfer-done", serde_json::json!({
                "session_id": session_id,
                "transfer_id": transfer_id,
                "filename": fname,
                "success": true,
                "bytes": result.bytes_transferred,
                "checksum": cksum,
                "avg_bytes_per_second": avg_bps,
                "is_server": false,
            }));
            Ok(())
        }
        Ok(Err(e)) => {
            let _ = app.emit("tftp-transfer-done", serde_json::json!({
                "session_id": session_id,
                "transfer_id": transfer_id,
                "filename": fname,
                "success": false,
                "error": e,
                "is_server": false,
            }));
            Err(e)
        }
        Err(e) => {
            let msg = format!("TFTP PUT panic: {}", e);
            let _ = app.emit("tftp-transfer-done", serde_json::json!({
                "session_id": session_id,
                "transfer_id": transfer_id,
                "filename": fname,
                "success": false,
                "error": msg,
                "is_server": false,
            }));
            Err(msg)
        }
    }
}
