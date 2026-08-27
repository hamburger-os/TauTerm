from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
lib = ROOT / "src-tauri/src/lib.rs"
ci = ROOT / ".github/workflows/ci.yml"
self_path = Path(__file__)

text = lib.read_text(encoding="utf-8")
old = '                            log::info!("已清理 {} 个孤儿虚拟端口对 (socat)", orphan_count);'
new = '                            log::info!("已清理 {} 个遗留虚拟端点资源", orphan_count);'
if text.count(old) != 1:
    raise RuntimeError(f"runtime label: expected exactly one match, got {text.count(old)}")
text = text.replace(old, new, 1)
if 'virtual_port::socat' in text:
    raise RuntimeError('stale virtual_port::socat reference remains')
lib.write_text(text, encoding="utf-8")

workflow = ci.read_text(encoding="utf-8")
workflow = workflow.replace('permissions:\n  contents: write\n', 'permissions:\n  contents: read\n', 1)
start = workflow.index('  final-runtime-label-cleanup:\n')
end = workflow.index('  rust:\n', start)
workflow = workflow[:start] + workflow[end:]
ci.write_text(workflow, encoding="utf-8")

self_path.unlink()
print('runtime label cleaned; CI restored; migration script removed')
