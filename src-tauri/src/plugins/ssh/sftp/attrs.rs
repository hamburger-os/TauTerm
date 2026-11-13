//! SFTP 文件属性模型与 POSIX mode 解释。
//!
//! 目录项类型判断、权限字符串渲染只有这一份实现；文件服务、传输和远程文档共用。

use serde::Serialize;

/// 远端目录项类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SftpEntryType {
    File,
    Directory,
    Symlink,
    Fifo,
    Socket,
    BlockDevice,
    CharDevice,
    Other,
}

/// 远端目录项 / 文件信息。
#[derive(Debug, Clone, Serialize)]
pub struct SftpEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub entry_type: SftpEntryType,
    pub size: u64,
    pub accessed: Option<u64>,
    pub modified: Option<u64>,
    pub permissions: Option<String>,
}

/// 由 POSIX mode 判断目录项类型；服务端不返回 mode 时退化为 is_dir 提示。
pub fn entry_type_from_permissions(perm: Option<u32>, fallback_is_dir: bool) -> SftpEntryType {
    match perm.map(|p| p & 0o170000) {
        Some(0o040000) => SftpEntryType::Directory,
        Some(0o100000) => SftpEntryType::File,
        Some(0o120000) => SftpEntryType::Symlink,
        Some(0o010000) => SftpEntryType::Fifo,
        Some(0o140000) => SftpEntryType::Socket,
        Some(0o060000) => SftpEntryType::BlockDevice,
        Some(0o020000) => SftpEntryType::CharDevice,
        Some(0) | None if fallback_is_dir => SftpEntryType::Directory,
        Some(0) | None => SftpEntryType::File,
        _ => SftpEntryType::Other,
    }
}

/// 只有普通文件可以进入文档/编辑工作流。
pub fn is_regular_file(perm: Option<u32>, fallback_is_dir: bool) -> bool {
    match perm.map(|value| value & 0o170000) {
        Some(0o100000) => true,
        Some(0) | None => !fallback_is_dir,
        _ => false,
    }
}

/// 渲染 `ls -l` 风格权限串（10 字符，含 setuid/setgid/sticky）。
pub fn permissions_to_string(perm: Option<u32>) -> String {
    let p = perm.unwrap_or(0);
    let mut s = String::with_capacity(10);
    s.push(match p & 0o170000 {
        0o040000 => 'd',
        0o120000 => 'l',
        0o010000 => 'p',
        0o140000 => 's',
        0o060000 => 'b',
        0o020000 => 'c',
        _ => '-',
    });
    s.push(if p & 0o400 != 0 { 'r' } else { '-' });
    s.push(if p & 0o200 != 0 { 'w' } else { '-' });
    s.push(match (p & 0o4000 != 0, p & 0o100 != 0) {
        (true, true) => 's',
        (true, false) => 'S',
        (false, true) => 'x',
        (false, false) => '-',
    });
    s.push(if p & 0o040 != 0 { 'r' } else { '-' });
    s.push(if p & 0o020 != 0 { 'w' } else { '-' });
    s.push(match (p & 0o2000 != 0, p & 0o010 != 0) {
        (true, true) => 's',
        (true, false) => 'S',
        (false, true) => 'x',
        (false, false) => '-',
    });
    s.push(if p & 0o004 != 0 { 'r' } else { '-' });
    s.push(if p & 0o002 != 0 { 'w' } else { '-' });
    s.push(match (p & 0o1000 != 0, p & 0o001 != 0) {
        (true, true) => 't',
        (true, false) => 'T',
        (false, true) => 'x',
        (false, false) => '-',
    });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symlink_is_not_a_regular_file() {
        assert!(!is_regular_file(Some(0o120777), false));
        assert!(is_regular_file(Some(0o100644), false));
        assert!(!is_regular_file(Some(0o040755), true));
    }

    #[test]
    fn permission_string_matches_ls_shape() {
        assert_eq!(permissions_to_string(Some(0o100644)), "-rw-r--r--");
        assert_eq!(permissions_to_string(Some(0o40755)), "drwxr-xr-x");
        assert_eq!(permissions_to_string(Some(0o104755)), "-rwsr-xr-x");
    }
}
