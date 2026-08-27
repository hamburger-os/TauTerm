from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]

def read(path):
    return (ROOT / path).read_text(encoding="utf-8")

def write(path, text):
    (ROOT / path).write_text(text, encoding="utf-8")

def once(text, old, new, label):
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected 1 match, got {count}")
    return text.replace(old, new, 1)

cargo = read("src-tauri/Cargo.toml")
cargo = once(cargo, 'directories = "6"\n', 'directories = "6"\natomic-write-file = "0.3.1"\n', "atomic-write dependency")
write("src-tauri/Cargo.toml", cargo)

path = "src-tauri/src/security/credential_store.rs"
s = read(path)
s = once(s, 'use std::fs;\nuse std::path::{Path, PathBuf};\n', 'use std::fs;\nuse std::io::Write;\nuse std::path::{Path, PathBuf};\n', "Write import")
old = '''fn private_atomic_write(path: &Path, b: &[u8]) -> Result<(), CredentialStoreError> {\n    let p = path.parent().ok_or_else(|| be("vault has no parent"))?;\n    fs::create_dir_all(p).map_err(be)?;\n    let t = p.join(format!(".{VAULT}.{}.tmp", uuid::Uuid::new_v4().simple()));\n    fs::write(&t, b).map_err(be)?;\n    #[cfg(unix)]\n    {\n        use std::os::unix::fs::PermissionsExt;\n        fs::set_permissions(&t, fs::Permissions::from_mode(0o600)).map_err(be)?;\n    }\n    #[cfg(target_os = "windows")]\n    if path.exists() {\n        fs::remove_file(path).map_err(be)?;\n    }\n    fs::rename(t, path).map_err(be)\n}\n'''
new = '''fn private_atomic_write(path: &Path, b: &[u8]) -> Result<(), CredentialStoreError> {\n    let parent = path.parent().ok_or_else(|| be("vault has no parent"))?;\n    fs::create_dir_all(parent).map_err(be)?;\n\n    let mut options = atomic_write_file::OpenOptions::new();\n    #[cfg(unix)]\n    {\n        use std::os::unix::fs::OpenOptionsExt as _;\n        options.mode(0o600);\n        use atomic_write_file::unix::OpenOptionsExt as _;\n        options.preserve_mode(true);\n    }\n\n    let mut file = options.open(path).map_err(be)?;\n    file.write_all(b).map_err(be)?;\n    file.commit().map_err(be)\n}\n'''
s = once(s, old, new, "atomic vault writer")
write(path, s)
print("final atomic review applied")
