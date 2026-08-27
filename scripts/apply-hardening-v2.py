from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def r(p): return (ROOT/p).read_text(encoding="utf-8")
def w(p,s): (ROOT/p).write_text(s,encoding="utf-8")
def once(s,a,b,label):
    if s.count(a)!=1: raise RuntimeError(f"{label}: expected 1, got {s.count(a)}")
    return s.replace(a,b,1)

# --- VirtualEndpoint migration ------------------------------------------------
paths=list((ROOT/'src-tauri/src').rglob('*.rs'))+list((ROOT/'src').rglob('*.ts'))+list((ROOT/'src').rglob('*.tsx'))
for p in paths:
    s=p.read_text(encoding='utf-8')
    for a,b in [
        ('PortPair','VirtualEndpoint'),('create_pairs_elevated','create_endpoints_elevated'),
        ('create_pairs','create_endpoints'),('destroy_pair','destroy_endpoint'),
        ('cleanup_pairs_elevated','cleanup_endpoints_elevated'),('active_pairs','active_endpoints'),
        ('virtual_port_pairs','virtual_endpoints'),('virtualPortPairs','virtualEndpoints'),
        ('vport_pairs_json','vport_endpoints_json'),('port_a','bridge_path'),
        ('port_b','external_path'),('bus_number','resource_id')]: s=s.replace(a,b)
    p.write_text(s,encoding='utf-8')

old=ROOT/'src-tauri/src/virtual_port/socat.rs'; new=ROOT/'src-tauri/src/virtual_port/pty.rs'
if old.exists(): old.rename(new)
for p in list((ROOT/'src-tauri/src').rglob('*.rs')):
    s=p.read_text(encoding='utf-8').replace('virtual_port::socat::PtyBackend','virtual_port::pty::PtyBackend').replace('pub mod socat;','pub mod pty;').replace('super::socat::','super::pty::').replace('SocatBackend','PtyBackend')
    s=s.replace('socat 已就绪，虚拟串口功能可用','原生 PTY 后端已就绪，虚拟串口功能可用').replace('socat 未安装，虚拟串口功能不可用。安装: apt install socat (Linux) / brew install socat (macOS)','原生 PTY 后端不可用').replace('socat not installed. Install via: sudo apt install socat (Linux) or brew install socat (macOS)','Native PTY backend unavailable')
    p.write_text(s,encoding='utf-8')

# Keep bridge_path internal in frontend payloads.
s=r('src-tauri/src/commands.rs')
s=s.replace('"bridge_path": p.bridge_path,\n                    "external_path": p.external_path,','"external_path": p.external_path,').replace('"pairs": &vport_endpoints_json,','"endpoints": &vport_endpoints_json,')
w('src-tauri/src/commands.rs',s)

# --- Dependencies -------------------------------------------------------------
c=r('src-tauri/Cargo.toml')
if 'keyring =' not in c:
    c=once(c,'crc32fast = "1.5.0"\n','crc32fast = "1.5.0"\nkeyring = "4.1.6"\naes-gcm = "0.11"\nargon2 = "0.5"\nbase64 = "0.22"\ngetrandom = "0.3"\nzeroize = "1.8"\ndirectories = "6"\n','cargo deps')
if '[dev-dependencies]' not in c: c+='\n[dev-dependencies]\ntempfile = "3"\n'
elif 'tempfile =' not in c: c=c.replace('[dev-dependencies]','[dev-dependencies]\ntempfile = "3"',1)
w('src-tauri/Cargo.toml',c)

# --- Credential store ---------------------------------------------------------
cred=r'''//! Persistent credential storage: native OS keyring first, authenticated encrypted vault fallback.
use std::collections::{BTreeMap,BTreeSet};
use std::fs;
use std::path::{Path,PathBuf};
use std::sync::Mutex;
use aes_gcm::aead::{Aead,KeyInit,Payload};
use aes_gcm::{Aes256Gcm,Nonce};
use argon2::{Algorithm,Argon2,Params,Version};
use base64::{engine::general_purpose::STANDARD as B64,Engine};
use directories::ProjectDirs;
use serde::{Deserialize,Serialize};
use zeroize::{Zeroize,Zeroizing};

const SERVICE:&str="com.tauterm.desktop.credentials";
const INDEX:&str="__tauterm_index_v1";
const VAULT:&str="credentials.vault.json";
const VERSION_NO:u32=1;
const M_COST:u32=64*1024; const T_COST:u32=3; const P_COST:u32=1;
const SALT:usize=16; const NONCE_LEN:usize=12;

#[derive(Debug,Clone,Serialize,Deserialize,PartialEq)] pub enum CredentialType{Password,SshKey,Certificate,Token}
#[derive(Debug,Clone,Serialize,Deserialize)] pub struct CredentialEntry{pub account:String,pub credential_type:CredentialType,pub description:String}
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(tag="kind",content="value",rename_all="snake_case")]
pub enum CredentialValue{Password(String),SshKey{private_key:String,passphrase:Option<String>},Certificate{cert_data:Vec<u8>,key_data:Vec<u8>},Token(String)}
#[derive(Debug,Clone,Serialize,Deserialize)] struct Stored{entry:CredentialEntry,value:CredentialValue}
#[derive(Debug,Clone,Serialize)] pub struct CredentialStorageStatus{pub backend:&'static str,pub native_available:bool,pub fallback_configured:bool,pub fallback_unlocked:bool}
#[derive(Debug,Serialize,Deserialize)] struct Kdf{algorithm:String,salt_b64:String,memory_kib:u32,iterations:u32,lanes:u32}
#[derive(Debug,Serialize,Deserialize)] struct Cipher{algorithm:String,nonce_b64:String}
#[derive(Debug,Serialize,Deserialize)] struct Envelope{version:u32,kdf:Kdf,cipher:Cipher,ciphertext_b64:String}
#[derive(Default)] struct Runtime{key:Option<Zeroizing<Vec<u8>>>}

pub struct CredentialStore{runtime:Mutex<Runtime>,vault_path:PathBuf}
impl Default for CredentialStore{fn default()->Self{Self::new()}}
impl CredentialStore{
 pub fn new()->Self{let d=ProjectDirs::from("com","TauTerm","TauTerm").map(|x|x.data_local_dir().to_path_buf()).unwrap_or_else(||PathBuf::from("."));Self::with_data_dir(d)}
 pub fn with_data_dir(d:PathBuf)->Self{Self{runtime:Mutex::new(Runtime::default()),vault_path:d.join(VAULT)}}
 pub fn status(&self)->CredentialStorageStatus{let n=Self::native_available();let u=self.runtime.lock().map(|x|x.key.is_some()).unwrap_or(false);CredentialStorageStatus{backend:if n{"native_keyring"}else{"encrypted_vault"},native_available:n,fallback_configured:self.vault_path.exists(),fallback_unlocked:u}}
 pub fn unlock_fallback(&self,password:&str)->Result<(),CredentialStoreError>{if Self::native_available(){return Err(CredentialStoreError::Backend("系统原生安全凭据存储可用，无需 fallback vault".into()))} if password.chars().count()<10{return Err(CredentialStoreError::Backend("主密码至少 10 个字符".into()))} if let Some(p)=self.vault_path.parent(){fs::create_dir_all(p).map_err(be)?} let retained=if self.vault_path.exists(){let e=self.read_env()?;let salt=dec_fixed::<SALT>(&e.kdf.salt_b64,"salt")?;let mut k=derive(password,&salt,&e.kdf)?;decrypt(&e,&k).map_err(|_|CredentialStoreError::InvalidMasterPassword)?;let z=Zeroizing::new(k.to_vec());k.zeroize();z}else{let mut salt=[0u8;SALT];getrandom::fill(&mut salt).map_err(|e|CredentialStoreError::Backend(e.to_string()))?;let kdf=Kdf{algorithm:"argon2id".into(),salt_b64:B64.encode(salt),memory_kib:M_COST,iterations:T_COST,lanes:P_COST};let mut k=derive(password,&salt,&kdf)?;self.write_map(&BTreeMap::new(),&k,kdf)?;let z=Zeroizing::new(k.to_vec());k.zeroize();z};self.runtime.lock().map_err(|_|CredentialStoreError::LockError)?.key=Some(retained);Ok(())}
 pub fn lock_fallback(&self){if let Ok(mut s)=self.runtime.lock(){s.key=None}}
 pub fn store_credential(&self,account:&str,credential_type:CredentialType,value:CredentialValue,description:&str)->Result<(),CredentialStoreError>{valid_account(account)?;let stored=Stored{entry:CredentialEntry{account:account.into(),credential_type,description:description.into()},value};if Self::native_available(){self.native_store(account,&stored)}else{self.with_map_mut(|m|{m.insert(account.into(),stored);Ok(())})}}
 pub fn get_credential(&self,account:&str)->Result<CredentialValue,CredentialStoreError>{valid_account(account)?;if Self::native_available(){self.native_get(account)?.map(|s|s.value).ok_or_else(||CredentialStoreError::NotFound(account.into()))}else{self.with_map(|m|m.get(account).map(|s|s.value.clone()).ok_or_else(||CredentialStoreError::NotFound(account.into())))}}
 pub fn list_credentials(&self)->Result<Vec<CredentialEntry>,CredentialStoreError>{if Self::native_available(){let mut out=Vec::new();for a in self.read_index()?{if let Some(s)=self.native_get(&a)?{out.push(s.entry)}}Ok(out)}else{self.with_map(|m|Ok(m.values().map(|s|s.entry.clone()).collect()))}}
 pub fn delete_credential(&self,account:&str)->Result<(),CredentialStoreError>{valid_account(account)?;if Self::native_available(){let e=Self::entry(account)?;match e.delete_credential(){Ok(())|Err(keyring::Error::NoEntry)=>{},Err(e)=>return Err(be(e))};let mut i=self.read_index()?;if i.remove(account){self.write_index(&i)?}Ok(())}else{self.with_map_mut(|m|{m.remove(account);Ok(())})}}
 fn native_available()->bool{keyring::Entry::store_status().is_ok()}
 fn entry(a:&str)->Result<keyring::Entry,CredentialStoreError>{keyring::Entry::new(SERVICE,a).map_err(be)}
 fn native_store(&self,a:&str,s:&Stored)->Result<(),CredentialStoreError>{let mut b=serde_json::to_vec(s).map_err(be)?;let e=Self::entry(a)?;e.set_secret(&b).map_err(be)?;b.zeroize();let mut i=self.read_index()?;if i.insert(a.into()){if let Err(x)=self.write_index(&i){let _=e.delete_credential();return Err(x)}}Ok(())}
 fn native_get(&self,a:&str)->Result<Option<Stored>,CredentialStoreError>{match Self::entry(a)?.get_secret(){Ok(mut b)=>{let x=serde_json::from_slice(&b).map_err(be);b.zeroize();x.map(Some)},Err(keyring::Error::NoEntry)=>Ok(None),Err(e)=>Err(be(e))}}
 fn read_index(&self)->Result<BTreeSet<String>,CredentialStoreError>{match Self::entry(INDEX)?.get_secret(){Ok(mut b)=>{let x=serde_json::from_slice(&b).map_err(be);b.zeroize();x},Err(keyring::Error::NoEntry)=>Ok(BTreeSet::new()),Err(e)=>Err(be(e))}}
 fn write_index(&self,i:&BTreeSet<String>)->Result<(),CredentialStoreError>{let mut b=serde_json::to_vec(i).map_err(be)?;let x=Self::entry(INDEX)?.set_secret(&b).map_err(be);b.zeroize();x}
 fn key(&self)->Result<Zeroizing<Vec<u8>>,CredentialStoreError>{self.runtime.lock().map_err(|_|CredentialStoreError::LockError)?.key.as_ref().map(|x|Zeroizing::new(x.to_vec())).ok_or(CredentialStoreError::VaultLocked)}
 fn with_map<T>(&self,f:impl FnOnce(&BTreeMap<String,Stored>)->Result<T,CredentialStoreError>)->Result<T,CredentialStoreError>{let k=self.key()?;let e=self.read_env()?;let m=decrypt(&e,&k)?;f(&m)}
 fn with_map_mut<T>(&self,f:impl FnOnce(&mut BTreeMap<String,Stored>)->Result<T,CredentialStoreError>)->Result<T,CredentialStoreError>{let k=self.key()?;let e=self.read_env()?;let mut m=decrypt(&e,&k)?;let x=f(&mut m)?;self.write_map(&m,&k,e.kdf)?;Ok(x)}
 fn read_env(&self)->Result<Envelope,CredentialStoreError>{let b=fs::read(&self.vault_path).map_err(be)?;let e:Envelope=serde_json::from_slice(&b).map_err(be)?;validate_env(&e)?;Ok(e)}
 fn write_map(&self,m:&BTreeMap<String,Stored>,key:&[u8],kdf:Kdf)->Result<(),CredentialStoreError>{let mut p=serde_json::to_vec(m).map_err(be)?;let mut n=[0u8;NONCE_LEN];getrandom::fill(&mut n).map_err(|e|CredentialStoreError::Backend(e.to_string()))?;let c=Aes256Gcm::new_from_slice(key).map_err(|_|CredentialStoreError::Backend("invalid vault key".into()))?;let aad=aad(&kdf);let ct=c.encrypt(Nonce::from_slice(&n),Payload{msg:&p,aad:aad.as_bytes()}).map_err(|_|CredentialStoreError::Backend("vault encryption failed".into()))?;p.zeroize();let e=Envelope{version:VERSION_NO,kdf,cipher:Cipher{algorithm:"aes-256-gcm".into(),nonce_b64:B64.encode(n)},ciphertext_b64:B64.encode(ct)};private_atomic_write(&self.vault_path,&serde_json::to_vec_pretty(&e).map_err(be)?) }
}
fn be(e:impl std::fmt::Display)->CredentialStoreError{CredentialStoreError::Backend(e.to_string())}
fn valid_account(a:&str)->Result<(),CredentialStoreError>{if a.trim().is_empty()||a.len()>512||a.contains('\0')||a==INDEX{Err(CredentialStoreError::Backend("invalid credential account".into()))}else{Ok(())}}
fn validate_env(e:&Envelope)->Result<(),CredentialStoreError>{if e.version!=VERSION_NO||e.kdf.algorithm!="argon2id"||e.cipher.algorithm!="aes-256-gcm"{return Err(be("unsupported vault format"))} validate_kdf(&e.kdf)}
fn validate_kdf(k:&Kdf)->Result<(),CredentialStoreError>{if !(32*1024..=1024*1024).contains(&k.memory_kib)||!(1..=10).contains(&k.iterations)||!(1..=16).contains(&k.lanes){Err(be("unsafe vault KDF parameters"))}else{Ok(())}}
fn derive(password:&str,salt:&[u8;SALT],k:&Kdf)->Result<[u8;32],CredentialStoreError>{validate_kdf(k)?;let p=Params::new(k.memory_kib,k.iterations,k.lanes,Some(32)).map_err(be)?;let mut out=[0u8;32];Argon2::new(Algorithm::Argon2id,Version::V0x13,p).hash_password_into(password.as_bytes(),salt,&mut out).map_err(be)?;Ok(out)}
fn aad(k:&Kdf)->String{format!("TauTerm:credential-vault:v{}:argon2id:m={}:t={}:p={}",VERSION_NO,k.memory_kib,k.iterations,k.lanes)}
fn decrypt(e:&Envelope,key:&[u8])->Result<BTreeMap<String,Stored>,CredentialStoreError>{validate_env(e)?;let n=dec_fixed::<NONCE_LEN>(&e.cipher.nonce_b64,"nonce")?;let ct=B64.decode(&e.ciphertext_b64).map_err(be)?;let c=Aes256Gcm::new_from_slice(key).map_err(|_|be("invalid vault key"))?;let a=aad(&e.kdf);let mut p=c.decrypt(Nonce::from_slice(&n),Payload{msg:&ct,aad:a.as_bytes()}).map_err(|_|be("vault authentication failed"))?;let x=serde_json::from_slice(&p).map_err(be);p.zeroize();x}
fn dec_fixed<const N:usize>(s:&str,name:&str)->Result<[u8;N],CredentialStoreError>{B64.decode(s).map_err(be)?.try_into().map_err(|_|be(format!("invalid {name} length")))}
fn private_atomic_write(path:&Path,b:&[u8])->Result<(),CredentialStoreError>{let p=path.parent().ok_or_else(||be("vault has no parent"))?;fs::create_dir_all(p).map_err(be)?;let t=p.join(format!(".{VAULT}.{}.tmp",uuid::Uuid::new_v4().simple()));fs::write(&t,b).map_err(be)?;#[cfg(unix)]{use std::os::unix::fs::PermissionsExt;fs::set_permissions(&t,fs::Permissions::from_mode(0o600)).map_err(be)?;} #[cfg(target_os="windows")] if path.exists(){fs::remove_file(path).map_err(be)?;} fs::rename(t,path).map_err(be)}
#[derive(Debug,thiserror::Error)] pub enum CredentialStoreError{#[error("凭据 '{0}' 不存在")]NotFound(String),#[error("类型不匹配")]TypeMismatch,#[error("内部锁错误")]LockError,#[error("加密凭据 vault 尚未解锁")]VaultLocked,#[error("主密码错误或 vault 被篡改")]InvalidMasterPassword,#[error("凭据存储错误: {0}")]Backend(String)}

#[cfg(test)] mod tests{use super::*;#[test]fn aead_detects_tamper(){let d=tempfile::tempdir().unwrap();let s=CredentialStore::with_data_dir(d.path().into());let salt=[7u8;SALT];let kdf=Kdf{algorithm:"argon2id".into(),salt_b64:B64.encode(salt),memory_kib:32*1024,iterations:1,lanes:1};let key=derive("correct horse battery staple",&salt,&kdf).unwrap();let mut m=BTreeMap::new();m.insert("x".into(),Stored{entry:CredentialEntry{account:"x".into(),credential_type:CredentialType::Password,description:"".into()},value:CredentialValue::Password("secret".into())});s.write_map(&m,&key,kdf).unwrap();let mut e=s.read_env().unwrap();assert!(decrypt(&e,&key).is_ok());let mut ct=B64.decode(&e.ciphertext_b64).unwrap();ct[0]^=1;e.ciphertext_b64=B64.encode(ct);assert!(decrypt(&e,&key).is_err());}}
'''
w('src-tauri/src/security/credential_store.rs',cred)

# commands and exports
s=r('src-tauri/src/commands.rs')
if 'credential_storage_status' not in s:
    marker='// ── ConfigStore 命令 ────────────────────────────────'
    add='''#[tauri::command]\npub fn credential_storage_status(state: State<'_, AppState>) -> Result<crate::security::credential_store::CredentialStorageStatus, String> { Ok(state.credential_store.status()) }\n\n#[tauri::command]\npub fn unlock_credential_vault(state: State<'_, AppState>, master_password: String) -> Result<(), String> { state.credential_store.unlock_fallback(&master_password).map_err(|e| e.to_string()) }\n\n#[tauri::command]\npub fn lock_credential_vault(state: State<'_, AppState>) -> Result<(), String> { state.credential_store.lock_fallback(); Ok(()) }\n\n'''
    s=once(s,marker,add+marker,'credential commands')
w('src-tauri/src/commands.rs',s)
s=r('src-tauri/src/lib.rs')
if 'commands::credential_storage_status' not in s:s=s.replace('commands::delete_credential,','commands::delete_credential,\n            commands::credential_storage_status,\n            commands::unlock_credential_vault,\n            commands::lock_credential_vault,',1)
w('src-tauri/src/lib.rs',s)

# settings UI
panel=ROOT/'src/components/Settings/panels/SecuritySettings.tsx'
panel.write_text('''import { useCallback, useEffect, useState } from "react";\nimport { invoke } from "@tauri-apps/api/core";\nimport { useTranslation } from "react-i18next";\ninterface S{backend:string;native_available:boolean;fallback_configured:boolean;fallback_unlocked:boolean}\nexport default function SecuritySettings(){const{t}=useTranslation();const[s,setS]=useState<S|null>(null);const[p,setP]=useState("");const[m,setM]=useState("");const refresh=useCallback(async()=>{try{setS(await invoke<S>("credential_storage_status"))}catch(e){setM(String(e))}},[]);useEffect(()=>{void refresh()},[refresh]);const unlock=async()=>{try{await invoke("unlock_credential_vault",{masterPassword:p});setP("");setM(t("settings.securityVaultUnlocked",{defaultValue:"Encrypted credential vault unlocked for this app session."}));await refresh()}catch(e){setM(String(e))}};const lock=async()=>{await invoke("lock_credential_vault");setP("");await refresh()};return <div style={{display:"grid",gap:16,maxWidth:680}}><h2>{t("settings.security",{defaultValue:"Security"})}</h2><p>{s?.native_available?t("settings.securityNative",{defaultValue:"Credentials are stored in the operating system secure store (Credential Manager / Keychain / Secret Service)."}):t("settings.securityFallback",{defaultValue:"The OS secure store is unavailable. Unlock the Argon2id + AES-256-GCM vault before storing credentials."})}</p>{s&&<p><strong>{t("settings.securityBackend",{defaultValue:"Active backend"})}:</strong> {s.backend}</p>}{s&&!s.native_available&&<><input className="liquid-glass-input" type="password" autoComplete="current-password" value={p} onChange={e=>setP(e.target.value)} placeholder={t("settings.securityMasterPassword",{defaultValue:"Master password (10+ characters)"})}/><div style={{display:"flex",gap:8}}><button className="liquid-glass-button" disabled={p.length<10} onClick={()=>void unlock()}>{t("settings.securityUnlock",{defaultValue:"Unlock vault"})}</button><button className="liquid-glass-button" disabled={!s.fallback_unlocked} onClick={()=>void lock()}>{t("settings.securityLock",{defaultValue:"Lock vault"})}</button></div></>}{m&&<p role="status">{m}</p>}</div>}\n''',encoding='utf-8')
s=r('src/components/Settings/SettingsPage.tsx')
if 'SecuritySettings' not in s:
 s=s.replace('import ShortcutSettings from "./panels/ShortcutSettings";','import ShortcutSettings from "./panels/ShortcutSettings";\nimport SecuritySettings from "./panels/SecuritySettings";').replace('type Category = "appearance" | "language" | "logging" | "shortcuts" | "about";','type Category = "appearance" | "language" | "logging" | "security" | "shortcuts" | "about";').replace('{ id: "logging", icon: "log" as const, labelKey: "settings.logging" },','{ id: "logging", icon: "log" as const, labelKey: "settings.logging" },\n  { id: "security", icon: "lock" as const, labelKey: "settings.security" },').replace('case "logging": return <LoggingSettings />;','case "logging": return <LoggingSettings />;\n      case "security": return <SecuritySettings />;')
w('src/components/Settings/SettingsPage.tsx',s)

# --- TFTP containment and explicit exposure gate ------------------------------
s=r('src-tauri/src/plugins/tftp/mod.rs')
if 'exposure_confirmed' not in s:
 s=s.replace('pub single_port: bool,','pub single_port: bool,\n    #[serde(default)]\n    pub exposure_confirmed: bool,')
 s=s.replace('if is_risky_exposure(listen_ip, &config) {','if is_risky_exposure(listen_ip, &config) && !config.exposure_confirmed {\n            return Err(SessionError::ConnectionFailed { reason: "TFTP 对非 loopback 网络开放远程写入并允许覆盖；需要用户明确确认".into() });\n        }\n        if is_risky_exposure(listen_ip, &config) {',1)
w('src-tauri/src/plugins/tftp/mod.rs',s)

s=r('src-tauri/src/plugins/tftp/server.rs')
rr='''    if !validate_file_path(&full_path, root) {\n        log::warn!("[TFTP Server] 路径遍历攻击: {}", filename);\n        send_error(ErrorCode::AccessViolation, "access violation");\n        return;\n    }\n    if !full_path.exists() {'''
rn='''    let full_path = match resolve_read_path(root, filename) { Ok(path) => path, Err(PathResolveError::NotFound) => { send_error(ErrorCode::FileNotFound, "file not found"); return; }, Err(error) => { log::warn!("[TFTP Server] 拒绝 RRQ 路径 {}: {:?}", filename, error); send_error(ErrorCode::AccessViolation, "access violation"); return; } };\n    if !full_path.exists() {'''
if rr in s:s=s.replace(rr,rn,1)
ww='''    if !validate_file_path(&full_path, root) {\n        log::warn!("[TFTP Server] 路径遍历攻击: {}", filename);\n        send_error(ErrorCode::AccessViolation, "access violation");\n        return;\n    }\n    if full_path.exists() && !overwrite {'''
wn='''    let full_path = match resolve_write_path(root, filename) { Ok(path) => path, Err(error) => { log::warn!("[TFTP Server] 拒绝 WRQ 路径 {}: {:?}", filename, error); send_error(ErrorCode::AccessViolation, "access violation"); return; } };\n    if full_path.exists() && !overwrite {'''
if ww in s:s=s.replace(ww,wn,1)
start=s.find('/// 清理文件名：')
if start<0:raise RuntimeError('TFTP helper marker missing')
s=s[:start]+'''#[derive(Debug,Clone,Copy,PartialEq,Eq)] enum PathResolveError{InvalidName,Escape,NotFound,Io}\nfn relative_request_path(filename:&str)->Result<PathBuf,PathResolveError>{let p=Path::new(filename);if p.as_os_str().is_empty(){return Err(PathResolveError::InvalidName)} for c in p.components(){match c{std::path::Component::Normal(_)|std::path::Component::CurDir=>{},_=>return Err(PathResolveError::InvalidName)}} Ok(p.to_path_buf())}\nfn resolve_read_path(root:&Path,filename:&str)->Result<PathBuf,PathResolveError>{let c=root.join(relative_request_path(filename)?);let p=c.canonicalize().map_err(|e|if e.kind()==std::io::ErrorKind::NotFound{PathResolveError::NotFound}else{PathResolveError::Io})?;if p.starts_with(root){Ok(p)}else{Err(PathResolveError::Escape)}}\nfn resolve_write_path(root:&Path,filename:&str)->Result<PathBuf,PathResolveError>{let c=root.join(relative_request_path(filename)?);if c.exists(){let p=c.canonicalize().map_err(|_|PathResolveError::Io)?;return if p.starts_with(root){Ok(p)}else{Err(PathResolveError::Escape)}}let parent=c.parent().ok_or(PathResolveError::InvalidName)?.canonicalize().map_err(|e|if e.kind()==std::io::ErrorKind::NotFound{PathResolveError::NotFound}else{PathResolveError::Io})?;if !parent.starts_with(root){return Err(PathResolveError::Escape)} Ok(parent.join(c.file_name().ok_or(PathResolveError::InvalidName)?))}\n#[cfg(test)] mod path_tests{use super::*;#[test]fn traversal_rejected_nested_allowed(){let d=tempfile::tempdir().unwrap();let root=d.path().canonicalize().unwrap();std::fs::create_dir(root.join("nested")).unwrap();std::fs::write(root.join("nested/a"),b"x").unwrap();assert_eq!(resolve_read_path(&root,"../x"),Err(PathResolveError::InvalidName));assert!(resolve_read_path(&root,"nested/a").is_ok());assert!(resolve_write_path(&root,"nested/b").is_ok());}#[cfg(unix)]#[test]fn symlink_escape_rejected(){use std::os::unix::fs::symlink;let d=tempfile::tempdir().unwrap();let o=tempfile::tempdir().unwrap();let root=d.path().canonicalize().unwrap();std::fs::write(o.path().join("secret"),b"x").unwrap();symlink(o.path(),root.join("escape")).unwrap();assert_eq!(resolve_read_path(&root,"escape/secret"),Err(PathResolveError::Escape));assert_eq!(resolve_write_path(&root,"escape/new"),Err(PathResolveError::Escape));}}\n'''
w('src-tauri/src/plugins/tftp/server.rs',s)

s=r('src/components/Layout/ConnectDialog.tsx')
if 'tftpExposureConfirmed' not in s:
 ins='''    let tftpExposureConfirmed = false;\n    if (isTftp && tftpWriteEnabled && tftpOverwrite) {\n      const bind = tftpListenIp.trim().toLowerCase();\n      const loopback = bind === "127.0.0.1" || bind === "::1" || bind === "localhost";\n      if (!loopback) {\n        tftpExposureConfirmed = window.confirm(t("tftp.exposureWarning", { defaultValue: "This TFTP server will accept remote writes and allow overwriting files from a non-loopback interface. Continue only on a trusted network." }));\n        if (!tftpExposureConfirmed) { setConnecting(false); return; }\n      }\n    }\n\n'''
 s=once(s,'    const params: Record<string, unknown> = isSerial ? {',ins+'    const params: Record<string, unknown> = isSerial ? {','TFTP UI gate').replace('      single_port: tftpSinglePort,','      single_port: tftpSinglePort,\n      exposure_confirmed: tftpExposureConfirmed,',1)
w('src/components/Layout/ConnectDialog.tsx',s)

# docs
for f in ['README.md','README.zh-CN.md']:
 s=r(f).replace('Credential persistence is currently in-memory only; OS-native keyring storage remains planned hardening work.','Credentials use the OS secure store when available, with an explicitly unlocked Argon2id + AES-256-GCM vault fallback.').replace('凭据当前仅保存在进程内存中；OS 原生 keyring 与加密持久化仍属于后续安全强化。','凭据优先使用系统原生安全存储；不可用时可显式解锁 Argon2id + AES-256-GCM 加密 vault。')
 w(f,s)
print('v2 migration applied')
