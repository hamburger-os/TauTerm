from pathlib import Path

path = Path(__file__).resolve().parents[1] / "src-tauri/src/transfer/zmodem.rs"
text = path.read_text(encoding="utf-8")
old = "/// 最大块大小\nconst MAX_BLOCK_SIZE: usize = 8192;\n"
if text.count(old) != 1:
    raise RuntimeError(f"expected one unused MAX_BLOCK_SIZE declaration, found {text.count(old)}")
path.write_text(text.replace(old, "", 1), encoding="utf-8")
print("removed obsolete ZMODEM MAX_BLOCK_SIZE constant")
