from pathlib import Path

path = Path('src-tauri/src/plugins/local_shell/elevated.rs')
text = path.read_text()
replacements = [
    ('                    shell.write_all(&payload)?;', '                    std::io::Write::write_all(&mut shell, &payload)?;'),
    ('                    shell.flush()?;', '                    std::io::Write::flush(&mut shell)?;'),
    ('        match shell.read(&mut buffer) {', '        match std::io::Read::read(&mut shell, &mut buffer) {'),
]
for old, new in replacements:
    if old not in text:
        raise SystemExit(f'missing anchor: {old!r}')
    text = text.replace(old, new, 1)
path.write_text(text)
