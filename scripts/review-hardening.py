from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def r(p): return (ROOT/p).read_text(encoding='utf-8')
def w(p,s): (ROOT/p).write_text(s,encoding='utf-8')
def once(s,a,b,label):
    n=s.count(a)
    if n!=1: raise RuntimeError(f'{label}: expected 1, got {n}')
    return s.replace(a,b,1)

# TFTP: remove obsolete lexical sanitizer calls; use only canonical resolvers.
s=r('src-tauri/src/plugins/tftp/server.rs')
legacy='    let file_path = sanitize_filename(filename);\n    let full_path = root.join(&file_path);\n\n'
if s.count(legacy)!=2: raise RuntimeError(f'expected 2 stale TFTP path initializers, got {s.count(legacy)}')
s=s.replace(legacy,'',2)
s=once(s,'match transfer::receive_file(&mut counting, full_path, remote, &params, abort, 1) {','match transfer::receive_file(&mut counting, full_path.clone(), remote, &params, abort, 1) {','WRQ retain resolved path')
old='''            if params.clean_on_error {\n                let file_path = root.join(sanitize_filename(filename));\n                let _ = std::fs::remove_file(&file_path);\n            }'''
new='''            if params.clean_on_error {\n                let _ = std::fs::remove_file(&full_path);\n            }'''
s=once(s,old,new,'WRQ cleanup canonical path')
if 'sanitize_filename' in s: raise RuntimeError('sanitize_filename residue remains')
w('src-tauri/src/plugins/tftp/server.rs',s)

# TFTP: backend independently enforces exposure confirmation; UI is not a trust boundary.
s=r('src-tauri/src/plugins/tftp/mod.rs')
needle='''        if let Some(warning) = exposure_warning(&config) {\n            log::warn!("[TFTP] {}", warning);\n        }'''
replace='''        if exposure_warning(&config).is_some() && !config.exposure_confirmed {\n            return Err(SessionError::ConnectionFailed {\n                reason: "TFTP is exposed to a non-loopback network with remote writes and overwrite enabled; explicit user confirmation is required".into(),\n            });\n        }\n        if let Some(warning) = exposure_warning(&config) {\n            log::warn!("[TFTP] confirmed exposure: {}", warning);\n        }'''
s=once(s,needle,replace,'TFTP backend exposure gate')
w('src-tauri/src/plugins/tftp/mod.rs',s)

# CredentialStore: never use CWD as a secret-vault fallback; supported desktop platforms must have app data.
s=r('src-tauri/src/security/credential_store.rs')
old='''        let d = ProjectDirs::from("com", "TauTerm", "TauTerm")\n            .map(|x| x.data_local_dir().to_path_buf())\n            .unwrap_or_else(|| PathBuf::from("."));'''
new='''        let d = ProjectDirs::from("com", "TauTerm", "TauTerm")\n            .expect("supported desktop platform must provide an application data directory")\n            .data_local_dir()\n            .to_path_buf();'''
s=once(s,old,new,'credential app data path')
# Preserve keyring/list index consistency if index read itself fails after secret creation.
old='''        let mut i = self.read_index()?;\n        if i.insert(a.into()) {'''
new='''        let mut i = match self.read_index() {\n            Ok(index) => index,\n            Err(error) => {\n                let _ = e.delete_credential();\n                return Err(error);\n            }\n        };\n        if i.insert(a.into()) {'''
s=once(s,old,new,'credential index rollback')
w('src-tauri/src/security/credential_store.rs',s)

# Zeroize the master-password command buffer when the IPC command returns.
s=r('src-tauri/src/commands.rs')
old='''pub fn unlock_credential_vault(\n    state: State<'_, AppState>,\n    master_password: String,\n) -> Result<(), String> {\n    state\n        .credential_store\n        .unlock_fallback(&master_password)\n        .map_err(|e| e.to_string())\n}'''
new='''pub fn unlock_credential_vault(\n    state: State<'_, AppState>,\n    master_password: String,\n) -> Result<(), String> {\n    let master_password = zeroize::Zeroizing::new(master_password);\n    state\n        .credential_store\n        .unlock_fallback(&master_password)\n        .map_err(|e| e.to_string())\n}'''
s=once(s,old,new,'master password zeroize')
w('src-tauri/src/commands.rs',s)

print('review hardening applied')
