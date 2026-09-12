from pathlib import Path
import re

root = Path('.')
protocol = Path('src-tauri/src/transfer/protocol.rs')
text = protocol.read_text(encoding='utf-8')
alias_block = '''/// 旧协议实现文件仍通过此内部别名实现 trait；新代码只应使用
/// `SerialTransferProtocol`。该别名可在协议文件逐步整理时无行为风险地移除。
pub use SerialTransferProtocol as TransferProtocol;

'''
if alias_block not in text:
    raise SystemExit('legacy TransferProtocol alias block not found')
protocol.write_text(text.replace(alias_block, '', 1), encoding='utf-8')

# Remove the exact compatibility-era trait name everywhere in implementation/docs without
# touching TransferProtocolType. X/Y/ZModem now implement SerialTransferProtocol directly.
paths = list(Path('src-tauri/src').rglob('*.rs')) + list(Path('docs').rglob('*.md'))
for path in paths:
    text = path.read_text(encoding='utf-8')
    updated = re.sub(r'\bTransferProtocol\b', 'SerialTransferProtocol', text)
    if updated != text:
        path.write_text(updated, encoding='utf-8')

file_transfer = Path('src-tauri/src/kernel/file_transfer.rs')
text = file_transfer.read_text(encoding='utf-8')
old = '- 串口 trait 绑定 `Box<dyn SerialPort>`，仅服务 X/Y/ZModem 协议算法'
new = '- 串口算法 trait 依赖协议无关的 `TransferIo = Read + Write + Send`，不绑定具体 serialport handle'
if old not in text:
    raise SystemExit('stale file-transfer comment anchor not found')
file_transfer.write_text(text.replace(old, new, 1), encoding='utf-8')

# The compatibility alias/name must be gone from product source and architecture docs.
remaining = []
pattern = re.compile(r'\bTransferProtocol\b')
for path in paths:
    for lineno, line in enumerate(path.read_text(encoding='utf-8').splitlines(), 1):
        if pattern.search(line):
            remaining.append(f'{path}:{lineno}:{line.strip()}')
if remaining:
    raise SystemExit('legacy TransferProtocol references remain:\n' + '\n'.join(remaining))
