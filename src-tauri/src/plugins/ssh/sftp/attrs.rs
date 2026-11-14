//! SFTP 文件属性模型与 POSIX mode 解释。
//!
//! 目录项类型判断、权限字符串渲染只有这一份实现；文件服务、传输和远程文档共用。

use serde::Serialize;

use super::error::SftpErrorCode;

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
///
/// `entry_type` 是目录项类型的唯一真源；目录、普通文件、符号链接和特殊文件都由它
/// 区分，不再维护 `is_dir` 这类派生布尔字段（两套真源是链接被漏判的根因）。
#[derive(Debug, Clone, Serialize)]
pub struct SftpEntry {
    pub name: String,
    pub path: String,
    pub entry_type: SftpEntryType,
    pub size: u64,
    pub accessed: Option<u64>,
    pub modified: Option<u64>,
    pub permissions: Option<String>,
}

/// 用户显式激活一条目录项时解析出的远端信息。
///
/// 分类（lstat）只是起点：符号链接的“跟随”属于用户操作语义，由本结构一次性
/// 给出目标原文、目标真实路径、目标类型和目标可用性。目标不可用不是 IPC 错误，
/// 而是 `target_exists == false` + `resolution_error_*`，这样 UI 能区分“链接断了”
/// 和“通道坏了”。跟随后的类型判定使用 SFTP `STAT`（follow），展示用的目标文本
/// 来自 `READLINK`，真实路径优先取 `REALPATH`。
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedEntry {
    /// 传入的原始路径（保留逻辑路径身份）。
    pub path: String,
    /// 原始路径自身的 lstat 类型；输入是普通文件/目录时等于 `target_type`。
    pub entry_type: SftpEntryType,
    /// `READLINK` 返回的目标原文（可能是相对路径）；非链接或服务端不支持时为 None。
    pub link_target: Option<String>,
    /// 目标的绝对路径：优先 `REALPATH`，不可用时按链接目录拼接目标原文。
    pub target_path: Option<String>,
    /// 跟随后的目标类型；目标不可解析时为 None。
    pub target_type: Option<SftpEntryType>,
    /// 跟随 `STAT` 是否成功。
    pub target_exists: bool,
    /// 跟随后的目标大小（目录/特殊文件为 None）。
    pub target_size: Option<u64>,
    /// 目标不可解析时的错误类别（NotFound / PermissionDenied / Loop / ...）。
    pub resolution_error_code: Option<SftpErrorCode>,
    /// 目标不可解析时的用户可见说明。
    pub resolution_error: Option<String>,
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
