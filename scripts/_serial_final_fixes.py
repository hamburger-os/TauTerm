from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]

def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")

def write(path: str, text: str) -> None:
    (ROOT / path).write_text(text, encoding="utf-8")

def exact(path: str, old: str, new: str, count: int = 1) -> None:
    text = read(path)
    actual = text.count(old)
    if actual != count:
        raise SystemExit(f"{path}: expected {count}, found {actual}: {old[:120]!r}")
    write(path, text.replace(old, new, count))

# Open every bridge endpoint synchronously before the bridge is registered in SessionStore.
# A requested multi-endpoint bridge is atomic: partial attachment is not considered success.
p = "src-tauri/src/virtual_port/bridge.rs"
old = '''        let subscription = io.subscribe().map_err(|error| error.to_string())?;
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_clone = cancel_flag.clone();

        let bridge_thread = std::thread::spawn(move || {
            if let Err(error) = bridge_loop(
                virtual_port_names,
                baud_rate,
                subscription,
                io,
                &cancel_clone,
            ) {
                log::error!("Virtual port bridge failed: {}", error);
                on_error(error);
            }
        });
'''
new = '''        let subscription = io.subscribe().map_err(|error| error.to_string())?;
        let mut virtual_ports: Vec<Box<dyn SerialPort>> = Vec::with_capacity(virtual_port_names.len());
        for name in &virtual_port_names {
            let port = open_bridge_endpoint(name, baud_rate)?;
            log::info!("Virtual endpoint {} attached to bridge", name);
            virtual_ports.push(port);
        }
        if virtual_ports.is_empty() {
            return Err("no virtual endpoints were available for bridging".into());
        }

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_clone = cancel_flag.clone();
        let bridge_thread = std::thread::spawn(move || {
            if let Err(error) = bridge_loop(virtual_ports, subscription, io, &cancel_clone) {
                log::error!("Virtual port bridge failed: {}", error);
                on_error(error);
            }
        });
'''
exact(p, old, new)
old = '''fn bridge_loop(
    virtual_port_names: Vec<String>,
    baud_rate: u32,
    subscription: crate::transport::DataPlaneSubscription,
    io: Arc<SessionIo>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut virtual_ports: Vec<Box<dyn SerialPort>> = Vec::new();
    for name in &virtual_port_names {
        match open_bridge_endpoint(name, baud_rate) {
            Ok(port) => {
                virtual_ports.push(port);
                log::info!("Virtual endpoint {} attached to bridge", name);
            }
            Err(error) => log::error!("{}", error),
        }
    }

    if virtual_ports.is_empty() {
        return Err("no virtual endpoints were available for bridging".into());
    }

'''
new = '''fn bridge_loop(
    mut virtual_ports: Vec<Box<dyn SerialPort>>,
    subscription: crate::transport::DataPlaneSubscription,
    io: Arc<SessionIo>,
    cancel: &AtomicBool,
) -> Result<(), String> {
'''
exact(p, old, new)

# Optional VPort setup must never leave unusable endpoint pairs registered or turn an otherwise
# valid physical Serial connection into a failed connect after the Session is already created.
p = "src-tauri/src/commands.rs"
text = read(p)
old = '''            let bridge = VirtualPortBridge::spawn(
                virtual_port_names,
                virtual_baud_rate,
                io,
                Box::new(move |reason| {
                    let _ = error_app.emit(
                        "virtual-port-failed",
                        serde_json::json!({
                            "session_id": error_session_id,
                            "kind": "bridge_failed",
                            "reason": reason,
                        }),
                    );
                }),
            )?;

            {
                let mut store = state
                    .session_store
                    .lock()
                    .map_err(|error| error.to_string())?;
                if let Some(handle) = store.get_session_mut(&session_id) {
                    handle.virtual_port_bridge = Some(bridge);
                    handle.virtual_endpoints = pairs.clone();
                }
            }

            let _ = app.emit(
                "virtual-port-created",
                serde_json::json!({
                    "session_id": session_id,
                    "endpoints": &vport_endpoints_json,
                }),
            );
'''
new = '''            match VirtualPortBridge::spawn(
                virtual_port_names,
                virtual_baud_rate,
                io,
                Box::new(move |reason| {
                    let _ = error_app.emit(
                        "virtual-port-failed",
                        serde_json::json!({
                            "session_id": error_session_id,
                            "kind": "bridge_failed",
                            "reason": reason,
                        }),
                    );
                }),
            ) {
                Ok(bridge) => {
                    {
                        let mut store = state
                            .session_store
                            .lock()
                            .map_err(|error| error.to_string())?;
                        if let Some(handle) = store.get_session_mut(&session_id) {
                            handle.virtual_port_bridge = Some(bridge);
                            handle.virtual_endpoints = pairs.clone();
                        }
                    }
                    let _ = app.emit(
                        "virtual-port-created",
                        serde_json::json!({
                            "session_id": session_id,
                            "endpoints": &vport_endpoints_json,
                        }),
                    );
                }
                Err(reason) => {
                    // VPort is an optional capability. Roll back only the endpoints that were just
                    // created; the already-established physical Serial session remains valid.
                    if let Ok(mut vpm) = state.virtual_port_manager.lock() {
                        for pair in &pairs {
                            let _ = vpm.destroy_endpoint(pair);
                        }
                    }
                    vport_endpoints_json.clear();
                    log::warn!(
                        "虚拟端口桥接启动失败 (session={}): {}",
                        session_id,
                        reason
                    );
                    let _ = app.emit(
                        "virtual-port-failed",
                        serde_json::json!({
                            "session_id": session_id,
                            "kind": "bridge_failed",
                            "reason": reason,
                        }),
                    );
                }
            }
'''
if text.count(old) != 1:
    raise SystemExit(f"commands.rs: expected one bridge spawn block, found {text.count(old)}")
write(p, text.replace(old, new, 1))

# Contract test for the bounded subscriber policy without depending on timing-sensitive hardware.
p = "src-tauri/src/transport/runtime.rs"
anchor = '''    #[test]
    fn exclusive_handoff_preserves_bytes_read_during_acquisition() {
'''
test = '''    #[test]
    fn slow_subscriber_is_detached_instead_of_blocking_or_growing_unbounded() {
        let (subscriber_tx, subscriber_rx) = mpsc::sync_channel(1);
        let mut state = RuntimeLoopState {
            subscribers: vec![(42, subscriber_tx)],
            startup_buffer: VecDeque::new(),
            startup_buffer_bytes: 0,
            handoff_buffer: VecDeque::new(),
            exclusive: None,
            shutdown_pending: false,
            tx_bytes: Arc::new(AtomicU64::new(0)),
            rx_bytes: Arc::new(AtomicU64::new(0)),
            exclusive_active: Arc::new(AtomicBool::new(false)),
        };

        publish_shared_data(&mut state, vec![1]);
        assert_eq!(state.subscribers.len(), 1);
        publish_shared_data(&mut state, vec![2]);
        assert!(state.subscribers.is_empty());
        match subscriber_rx.try_recv().unwrap() {
            DataPlaneEvent::Data(data) => assert_eq!(data, vec![1]),
            DataPlaneEvent::Closed(info) => panic!("unexpected close: {}", info.reason),
        }
    }

    #[test]
    fn exclusive_handoff_preserves_bytes_read_during_acquisition() {
'''
exact(p, anchor, test)

print("final serial lifecycle fixes applied")
