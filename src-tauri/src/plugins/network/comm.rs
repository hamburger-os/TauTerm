//! 网络调试会话的 CommHandle 实现。
//!
//! 网络容器会话（UDP / TCP）本身不直接持有 I/O 通道，发送必须按「当前目标」
//! 路由：UDP server 手动地址 / UDP client 固定远端 / TCP 选中对端或全部客户端。
//! 本实现把 `send()` 委托给 [`NetworkSideChannel`] 的 UDP 发送或 TCP 对端写通道
//! 注册表；同时承载脚本引擎的数据接收回调（UDP 报文 / TCP 对端数据经
//! `notify_receive` 送达 `on_data`）。
//!
//! 脚本/自动应答引擎统一绑定到容器会话，从而让 TCP 群发（「全部客户端」目标）
//! 与 UDP 手动目标在四个发送模式中都生效。

use std::sync::{mpsc, Arc};

use crate::channel::io_loop::IoLoopCmd;
use crate::kernel::comm_handle::{CommError, CommHandle, DataCallback};

use super::NetworkSideChannel;

pub struct NetworkCommHandle {
    side: Arc<NetworkSideChannel>,
    transport: String,
    role: String,
}

impl NetworkCommHandle {
    pub fn new(side: Arc<NetworkSideChannel>, transport: String, role: String) -> Self {
        Self {
            side,
            transport,
            role,
        }
    }
}

impl CommHandle for NetworkCommHandle {
    fn send(&self, data: &[u8]) -> Result<(), CommError> {
        match (self.transport.as_str(), self.role.as_str()) {
            ("udp", "server") => {
                let target = self
                    .side
                    .current_target()
                    .ok_or_else(|| CommError::SendError("无可用发送目标".to_string()))?;
                self.side
                    .udp_send_to(&target, data)
                    .map_err(CommError::SendError)
            }
            ("udp", "client") => self.side.udp_send(data).map_err(CommError::SendError),
            ("tcp", _) => {
                // TCP 容器级路由：按「当前目标」（选中对端 / 全部客户端 / 唯一对端）写入对端通道。
                // 先快照目标与发送端，锁外发送——避免持 peer_writers 锁阻塞在 send 上，
                // 与对端 on_disconnect（同样取该锁移除对端）形成死锁。
                let (target, senders): (
                    Option<String>,
                    Vec<(String, mpsc::SyncSender<IoLoopCmd>)>,
                ) = {
                    let writers = self
                        .side
                        .peer_writers
                        .lock()
                        .map_err(|e| CommError::SendError(e.to_string()))?;
                    let snap = writers
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    (self.side.current_target(), snap)
                };
                match target.as_deref() {
                    Some("__all__") => {
                        // 逐个发送，单个失败不中断其余对端（部分广播仍尽力送达）
                        let mut first_err: Option<CommError> = None;
                        for (_, tx) in &senders {
                            if let Err(e) = tx.send(IoLoopCmd::Write(data.to_vec())) {
                                if first_err.is_none() {
                                    first_err = Some(CommError::SendError(e.to_string()));
                                }
                            }
                        }
                        match first_err {
                            Some(e) => Err(e),
                            None => Ok(()),
                        }
                    }
                    Some(peer_id) => {
                        let tx = senders
                            .iter()
                            .find(|(id, _)| id == peer_id)
                            .map(|(_, tx)| tx)
                            .ok_or_else(|| {
                                CommError::SendError("目标对端不存在或已断开".to_string())
                            })?;
                        tx.send(IoLoopCmd::Write(data.to_vec()))
                            .map_err(|e| CommError::SendError(e.to_string()))
                    }
                    None => {
                        if senders.len() == 1 {
                            let (_, tx) = &senders[0];
                            tx.send(IoLoopCmd::Write(data.to_vec()))
                                .map_err(|e| CommError::SendError(e.to_string()))
                        } else {
                            Err(CommError::SendError("无可用发送目标".to_string()))
                        }
                    }
                }
            }
            _ => Err(CommError::SendError("不支持的传输类型".to_string())),
        }
    }

    fn send_text(&self, data: &[u8]) -> Result<Vec<u8>, CommError> {
        let encoding = self.side.encoding();
        let out = if encoding.eq_ignore_ascii_case("utf-8") {
            data.to_vec()
        } else {
            crate::kernel::charset::transcode_utf8_to_encoding(data, &encoding)
                .unwrap_or_else(|| data.to_vec())
        };
        self.send(&out)?;
        Ok(out)
    }

    fn send_to(&self, target: &str, data: &[u8]) -> Result<(), CommError> {
        if self.transport != "udp" {
            return Err(CommError::SendError(
                "仅 UDP 会话支持按目标地址发送".to_string(),
            ));
        }
        self.side
            .udp_send_to(target, data)
            .map_err(CommError::SendError)
    }

    fn send_to_text(&self, target: &str, data: &[u8]) -> Result<(), CommError> {
        let encoding = self.side.encoding();
        let out = if encoding.eq_ignore_ascii_case("utf-8") {
            data.to_vec()
        } else {
            crate::kernel::charset::transcode_utf8_to_encoding(data, &encoding)
                .unwrap_or_else(|| data.to_vec())
        };
        self.send_to(target, &out)
    }

    fn on_receive(&self, callback: DataCallback) {
        self.side.register_receiver(callback);
    }

    fn notify_receive(&self, data: &[u8]) {
        self.side.notify_receive(data);
    }

    fn clear_receivers(&self) {
        self.side.clear_receivers();
    }
}
