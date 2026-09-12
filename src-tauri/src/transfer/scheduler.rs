//! Session 级传输调度器。
//!
//! 默认每个 Session 只允许 1 个活动传输，但内部存储采用有界任务 Map，
//! 从数据模型上不再把“单任务”写死。Auxiliary 可按策略提升有界并发；
//! Inline 会接管 Session 主 I/O 资源，因此无论并发上限如何始终保持独占。

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::oneshot;

pub const DEFAULT_MAX_ACTIVE_PER_SESSION: usize = 1;

#[derive(Debug)]
enum TransferCancelSignal {
    Inline(Option<oneshot::Sender<()>>),
    Auxiliary(Arc<AtomicBool>),
}

#[derive(Debug)]
struct ScheduledTransfer {
    cancel: TransferCancelSignal,
}

#[derive(Debug)]
pub struct TransferScheduler {
    max_active: usize,
    active: HashMap<String, ScheduledTransfer>,
}

impl Default for TransferScheduler {
    fn default() -> Self {
        Self::with_max_active(DEFAULT_MAX_ACTIVE_PER_SESSION)
    }
}

impl TransferScheduler {
    pub fn with_max_active(max_active: usize) -> Self {
        Self {
            max_active,
            active: HashMap::new(),
        }
    }

    pub fn max_active(&self) -> usize {
        self.max_active
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn is_busy(&self) -> bool {
        !self.active.is_empty()
    }

    /// 仅当恰好有一个活动任务时返回其 ID。
    /// 多任务场景必须显式携带 transfer_id，避免“当前任务”歧义。
    pub fn active_id(&self) -> Option<&str> {
        if self.active.len() != 1 {
            return None;
        }
        self.active.keys().next().map(String::as_str)
    }

    fn ensure_new_id(&self, transfer_id: &str) -> Result<(), String> {
        if self.active.contains_key(transfer_id) {
            return Err("传输任务 ID 已存在".to_string());
        }
        Ok(())
    }

    fn has_inline_transfer(&self) -> bool {
        self.active
            .values()
            .any(|transfer| matches!(&transfer.cancel, TransferCancelSignal::Inline(_)))
    }

    fn ensure_auxiliary_capacity(&self) -> Result<(), String> {
        if self.has_inline_transfer() {
            return Err("该会话正在执行独占式串口传输，暂不能启动侧通道传输".to_string());
        }
        if self.max_active == 0 || self.active.len() >= self.max_active {
            return Err(format!(
                "该会话已有传输进行中（当前并发上限 {}），请等待完成或取消后再试",
                self.max_active
            ));
        }
        Ok(())
    }

    pub fn reserve_inline(
        &mut self,
        transfer_id: &str,
        cancel_tx: oneshot::Sender<()>,
    ) -> Result<(), String> {
        self.ensure_new_id(transfer_id)?;
        if self.max_active == 0 {
            return Err("该会话已禁用文件传输任务".to_string());
        }
        if !self.active.is_empty() {
            return Err("串口传输需要独占会话 I/O，请等待其他传输完成或取消后再试".to_string());
        }
        self.active.insert(
            transfer_id.to_string(),
            ScheduledTransfer {
                cancel: TransferCancelSignal::Inline(Some(cancel_tx)),
            },
        );
        Ok(())
    }

    pub fn reserve_auxiliary(&mut self, transfer_id: &str) -> Result<Arc<AtomicBool>, String> {
        self.ensure_new_id(transfer_id)?;
        self.ensure_auxiliary_capacity()?;
        let flag = Arc::new(AtomicBool::new(false));
        self.active.insert(
            transfer_id.to_string(),
            ScheduledTransfer {
                cancel: TransferCancelSignal::Auxiliary(flag.clone()),
            },
        );
        Ok(flag)
    }

    pub fn cancel(&mut self, expected_id: Option<&str>) -> Result<(), String> {
        let transfer_id = match expected_id {
            Some(id) => id.to_string(),
            None => {
                if self.active.is_empty() {
                    return Err("没有正在进行的传输".to_string());
                }
                if self.active.len() != 1 {
                    return Err("存在多个传输任务，请指定 transfer_id".to_string());
                }
                self.active
                    .keys()
                    .next()
                    .cloned()
                    .ok_or_else(|| "没有正在进行的传输".to_string())?
            }
        };

        let transfer = self
            .active
            .get_mut(&transfer_id)
            .ok_or_else(|| "传输任务已变化，拒绝取消非当前任务".to_string())?;

        match &mut transfer.cancel {
            TransferCancelSignal::Inline(tx) => {
                let tx = tx.take().ok_or_else(|| "取消请求已经发送".to_string())?;
                let _ = tx.send(());
            }
            TransferCancelSignal::Auxiliary(flag) => {
                flag.store(true, Ordering::SeqCst);
            }
        }
        Ok(())
    }

    pub fn finish(&mut self, expected_id: Option<&str>) -> bool {
        match expected_id {
            Some(id) => self.active.remove(id).is_some(),
            None => {
                if self.active.len() != 1 {
                    return false;
                }
                let Some(id) = self.active.keys().next().cloned() else {
                    return false;
                };
                self.active.remove(&id).is_some()
            }
        }
    }

    pub fn cancel_for_shutdown(&mut self) {
        for transfer in self.active.values_mut() {
            match &mut transfer.cancel {
                TransferCancelSignal::Inline(tx) => {
                    if let Some(tx) = tx.take() {
                        let _ = tx.send(());
                    }
                }
                TransferCancelSignal::Auxiliary(flag) => {
                    flag.store(true, Ordering::SeqCst);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_single_active_transfer() {
        let scheduler = TransferScheduler::default();
        assert_eq!(scheduler.max_active(), DEFAULT_MAX_ACTIVE_PER_SESSION);
        assert_eq!(scheduler.max_active(), 1);
        assert_eq!(scheduler.active_count(), 0);
        assert!(!scheduler.is_busy());
    }

    #[tokio::test]
    async fn inline_reservation_is_exactly_cancellable() {
        let (tx, rx) = oneshot::channel();
        let mut scheduler = TransferScheduler::default();
        scheduler
            .reserve_inline("transfer-a", tx)
            .expect("reserve inline");
        assert_eq!(scheduler.active_id(), Some("transfer-a"));
        assert!(scheduler.cancel(Some("transfer-b")).is_err());
        scheduler
            .cancel(Some("transfer-a"))
            .expect("cancel exact id");
        rx.await.expect("cancel signal");
        assert!(scheduler.finish(Some("transfer-a")));
        assert!(!scheduler.is_busy());
    }

    #[test]
    fn default_auxiliary_policy_rejects_second_active_task() {
        let mut scheduler = TransferScheduler::default();
        let flag = scheduler
            .reserve_auxiliary("transfer-a")
            .expect("reserve side channel");
        assert!(scheduler.reserve_auxiliary("transfer-b").is_err());
        scheduler.cancel(Some("transfer-a")).expect("cancel");
        assert!(flag.load(Ordering::SeqCst));
        assert!(scheduler.finish(Some("transfer-a")));
    }

    #[test]
    fn bounded_map_model_supports_future_auxiliary_concurrency() {
        let mut scheduler = TransferScheduler::with_max_active(2);
        let a = scheduler
            .reserve_auxiliary("transfer-a")
            .expect("reserve a");
        let b = scheduler
            .reserve_auxiliary("transfer-b")
            .expect("reserve b");
        assert_eq!(scheduler.active_count(), 2);
        assert_eq!(scheduler.active_id(), None);
        assert!(scheduler.reserve_auxiliary("transfer-c").is_err());
        assert!(scheduler.cancel(None).is_err());
        scheduler.cancel(Some("transfer-b")).expect("cancel b");
        assert!(!a.load(Ordering::SeqCst));
        assert!(b.load(Ordering::SeqCst));
        assert!(scheduler.finish(Some("transfer-a")));
        assert!(scheduler.finish(Some("transfer-b")));
        assert!(!scheduler.is_busy());
    }

    #[test]
    fn inline_is_exclusive_even_when_auxiliary_limit_is_higher() {
        let mut scheduler = TransferScheduler::with_max_active(2);
        scheduler
            .reserve_auxiliary("side-a")
            .expect("reserve side channel");
        let (inline_tx, _inline_rx) = oneshot::channel();
        assert!(scheduler.reserve_inline("inline-a", inline_tx).is_err());
        assert_eq!(scheduler.active_count(), 1);
    }

    #[test]
    fn auxiliary_cannot_start_while_inline_owns_session_io() {
        let mut scheduler = TransferScheduler::with_max_active(2);
        let (inline_tx, _inline_rx) = oneshot::channel();
        scheduler
            .reserve_inline("inline-a", inline_tx)
            .expect("reserve inline");
        assert!(scheduler.reserve_auxiliary("side-a").is_err());
        let (second_inline_tx, _second_inline_rx) = oneshot::channel();
        assert!(scheduler
            .reserve_inline("inline-b", second_inline_tx)
            .is_err());
        assert_eq!(scheduler.active_count(), 1);
    }
}
