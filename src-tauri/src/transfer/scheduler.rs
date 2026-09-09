//! Session 级传输调度器。
//!
//! 当前默认每个 Session 只允许 1 个活动传输，但准入、任务身份和取消信号集中
//! 在这里，而不是由 SessionHandle 平行保存若干 Option。以后扩展排队或多 SideChannel
//! 时只需要演进调度器，不需要改变文件管理器/编排器的任务身份契约。

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::oneshot;

pub const DEFAULT_MAX_ACTIVE_PER_SESSION: usize = 1;

#[derive(Debug)]
enum TransferCancelSignal {
    Inline(Option<oneshot::Sender<()>>),
    SideChannel(Arc<AtomicBool>),
}

#[derive(Debug)]
struct ScheduledTransfer {
    id: String,
    cancel: TransferCancelSignal,
}

#[derive(Debug)]
pub struct TransferScheduler {
    max_active: usize,
    active: Option<ScheduledTransfer>,
}

impl Default for TransferScheduler {
    fn default() -> Self {
        Self {
            max_active: DEFAULT_MAX_ACTIVE_PER_SESSION,
            active: None,
        }
    }
}

impl TransferScheduler {
    pub fn max_active(&self) -> usize {
        self.max_active
    }

    pub fn is_busy(&self) -> bool {
        self.active.is_some()
    }

    pub fn active_id(&self) -> Option<&str> {
        self.active.as_ref().map(|transfer| transfer.id.as_str())
    }

    fn ensure_capacity(&self) -> Result<(), String> {
        // 当前实现有一个活动槽；max_active 作为明确策略入口保留，未来可把 active
        // 升级为有界 Map/queue 而不改变调用方。
        if self.active.is_some() || self.max_active == 0 {
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
        self.ensure_capacity()?;
        self.active = Some(ScheduledTransfer {
            id: transfer_id.to_string(),
            cancel: TransferCancelSignal::Inline(Some(cancel_tx)),
        });
        Ok(())
    }

    pub fn reserve_side_channel(&mut self, transfer_id: &str) -> Result<Arc<AtomicBool>, String> {
        self.ensure_capacity()?;
        let flag = Arc::new(AtomicBool::new(false));
        self.active = Some(ScheduledTransfer {
            id: transfer_id.to_string(),
            cancel: TransferCancelSignal::SideChannel(flag.clone()),
        });
        Ok(flag)
    }

    pub fn cancel(&mut self, expected_id: Option<&str>) -> Result<(), String> {
        let transfer = self
            .active
            .as_mut()
            .ok_or_else(|| "没有正在进行的传输".to_string())?;

        if let Some(expected) = expected_id {
            if transfer.id != expected {
                return Err("传输任务已变化，拒绝取消非当前任务".to_string());
            }
        }

        match &mut transfer.cancel {
            TransferCancelSignal::Inline(tx) => {
                let tx = tx
                    .take()
                    .ok_or_else(|| "取消请求已经发送".to_string())?;
                let _ = tx.send(());
            }
            TransferCancelSignal::SideChannel(flag) => {
                flag.store(true, Ordering::SeqCst);
            }
        }
        Ok(())
    }

    pub fn finish(&mut self, expected_id: Option<&str>) -> bool {
        let Some(active) = self.active.as_ref() else {
            return false;
        };
        if let Some(expected) = expected_id {
            if active.id != expected {
                return false;
            }
        }
        self.active = None;
        true
    }

    pub fn cancel_for_shutdown(&mut self) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        match &mut active.cancel {
            TransferCancelSignal::Inline(tx) => {
                if let Some(tx) = tx.take() {
                    let _ = tx.send(());
                }
            }
            TransferCancelSignal::SideChannel(flag) => {
                flag.store(true, Ordering::SeqCst);
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
        scheduler.cancel(Some("transfer-a")).expect("cancel exact id");
        rx.await.expect("cancel signal");
        assert!(scheduler.finish(Some("transfer-a")));
        assert!(!scheduler.is_busy());
    }

    #[test]
    fn side_channel_reservation_rejects_second_active_task() {
        let mut scheduler = TransferScheduler::default();
        let flag = scheduler
            .reserve_side_channel("transfer-a")
            .expect("reserve side channel");
        assert!(scheduler.reserve_side_channel("transfer-b").is_err());
        scheduler.cancel(Some("transfer-a")).expect("cancel");
        assert!(flag.load(Ordering::SeqCst));
        assert!(scheduler.finish(Some("transfer-a")));
    }
}
