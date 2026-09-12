from pathlib import Path

session_store = Path('src-tauri/src/kernel/session_store.rs')
text = session_store.read_text(encoding='utf-8')
old = '''        crate::plugins::ssh::journald::stop_journald_stream(session_id);\n        crate::plugins::ssh::journald::stop_journald_export(session_id);\n'''
if old not in text:
    raise SystemExit('session_store journald cleanup anchor not found')
text = text.replace(old, '', 1)
session_store.write_text(text, encoding='utf-8')

ssh_mod = Path('src-tauri/src/plugins/ssh/mod.rs')
text = ssh_mod.read_text(encoding='utf-8')
old = '''    fn on_detached(&self, session_id: &str) {\n        if let Ok(mut map) = runtime_registry().lock() {\n            map.remove(session_id);\n        }\n    }\n'''
new = '''    fn on_detached(&self, session_id: &str) {\n        // SSH-owned background operations are keyed by the final Session id, so their\n        // lifecycle cleanup belongs in the SSH attachment hook rather than SessionStore.\n        journald::stop_journald_stream(session_id);\n        journald::stop_journald_export(session_id);\n        if let Ok(mut map) = runtime_registry().lock() {\n            map.remove(session_id);\n        }\n    }\n'''
if old not in text:
    raise SystemExit('SSH RuntimeAttach::on_detached anchor not found')
text = text.replace(old, new, 1)
ssh_mod.write_text(text, encoding='utf-8')
