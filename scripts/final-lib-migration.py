from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PATH = ROOT / "src-tauri/src/lib.rs"
text = PATH.read_text(encoding="utf-8")

def once(old: str, new: str, label: str) -> None:
    global text
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one match, got {count}")
    text = text.replace(old, new, 1)

once(
    'use virtual_port::socat::PtyBackend;',
    'use virtual_port::pty::PtyBackend;',
    'PtyBackend module import',
)
once(
    '    /// 虚拟串口设备管理器（com0com 驱动 + 端口对生命周期）',
    '    /// 虚拟端点后端（平台差异封装在 VirtualPortBackend 内）',
    'AppState endpoint abstraction comment',
)
once(
    '                        // 清理上次异常退出可能遗留的孤儿 symlink\n                        let orphan_count = vpm.cleanup_orphans();',
    '                        // 原生 PTY 随文件描述符自动释放；统一调用保持后端生命周期接口一致。\n                        let orphan_count = vpm.cleanup_orphans();',
    'Unix PTY lifecycle comment',
)

if 'virtual_port::socat' in text:
    raise RuntimeError('stale virtual_port::socat module reference remains')

PATH.write_text(text, encoding="utf-8")
print("final lib.rs endpoint migration applied")
