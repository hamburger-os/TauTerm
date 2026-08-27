from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def replace_once(path: str, old: str, new: str, label: str) -> None:
    p = ROOT / path
    text = p.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one match, got {count}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")

replace_once(
    "src-tauri/src/plugins/tftp/mod.rs",
    '''            overwrite: true,\n            single_port: false,\n        };''',
    '''            overwrite: true,\n            single_port: false,\n            exposure_confirmed: false,\n        };''',
    "TFTP exposure test initializer",
)

cred = ROOT / "src-tauri/src/security/credential_store.rs"
text = cred.read_text(encoding="utf-8")
old_encrypt = '''        let ct = c\n            .encrypt(\n                Nonce::from_slice(&n),\n                Payload {'''
new_encrypt = '''        let nonce = <&Nonce>::try_from(n.as_slice())\n            .map_err(|_| CredentialStoreError::Backend("invalid nonce length".into()))?;\n        let ct = c\n            .encrypt(\n                nonce,\n                Payload {'''
if text.count(old_encrypt) != 1:
    raise RuntimeError(f"encrypt nonce migration: expected 1 match, got {text.count(old_encrypt)}")
text = text.replace(old_encrypt, new_encrypt, 1)
old_decrypt = '''    let mut p = c\n        .decrypt(\n            Nonce::from_slice(&n),\n            Payload {'''
new_decrypt = '''    let nonce = <&Nonce>::try_from(n.as_slice()).map_err(|_| be("invalid nonce length"))?;\n    let mut p = c\n        .decrypt(\n            nonce,\n            Payload {'''
if text.count(old_decrypt) != 1:
    raise RuntimeError(f"decrypt nonce migration: expected 1 match, got {text.count(old_decrypt)}")
text = text.replace(old_decrypt, new_decrypt, 1)
if "Nonce::from_slice" in text:
    raise RuntimeError("deprecated Nonce::from_slice remains")
cred.write_text(text, encoding="utf-8")

Path(__file__).unlink()
print("final CI compile fixes applied; migration script removed")
