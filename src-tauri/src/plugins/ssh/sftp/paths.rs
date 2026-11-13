//! 远端路径工具。
//!
//! SFTP 路径始终是远端字节串语义，不能复用 `std::path`（它按本地平台解释分隔符）。
//! 这里只做与协议无关的字符串拼接与拆分，供文件服务、传输和远程文档共用。

/// 拆分远端路径为 (目录, 名称)。根目录 `/` 的目录部分为空串。
pub fn parts(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some((dir, name)) => (dir, name),
        None => ("", path),
    }
}

/// 拼接远端目录与名称，保留根目录 `/` 的单斜杠形式。
pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else if dir == "/" {
        format!("/{}", name)
    } else {
        format!("{}/{}", dir.trim_end_matches('/'), name)
    }
}

/// 把本地扫描出的相对路径拼到远端根目录，两侧多余的 `/` 被规范化。
pub fn join_relative(base: &str, relative: &str) -> String {
    let base = base.trim_end_matches('/');
    let relative = relative.trim_start_matches('/');
    if base.is_empty() {
        format!("/{}", relative)
    } else if relative.is_empty() {
        base.to_string()
    } else {
        format!("{}/{}", base, relative)
    }
}

/// 同名冲突时生成 Keep Both 候选名（保留扩展名位置）。
pub fn keep_both_candidate(path: &str, index: u32) -> String {
    let (dir, name) = parts(path);
    let dot = name.rfind('.').filter(|idx| *idx > 0);
    let next = match dot {
        Some(idx) => format!("{} ({}).{}", &name[..idx], index, &name[idx + 1..]),
        None => format!("{} ({})", name, index),
    };
    join(dir, &next)
}

/// 生成同目录临时产物名，用于事务式写入后的提交/回滚。
pub fn sibling_artifact(path: &str, tag: &str) -> String {
    let (dir, name) = parts(path);
    join(
        dir,
        &format!(".{}.tauterm-{}-{}", name, tag, uuid::Uuid::new_v4()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_preserves_root_and_relative_shapes() {
        assert_eq!(join("/", "a"), "/a");
        assert_eq!(join("/home/u", "a"), "/home/u/a");
        assert_eq!(join("rel", "a"), "rel/a");
        assert_eq!(join_relative("/home/u/", "/a/b"), "/home/u/a/b");
        assert_eq!(join_relative("/", "a/b"), "/a/b");
        assert_eq!(join_relative("/home/u", ""), "/home/u");
    }

    #[test]
    fn keep_both_keeps_extension() {
        assert_eq!(keep_both_candidate("/a/b.txt", 2), "/a/b (2).txt");
        assert_eq!(keep_both_candidate("/a/b", 2), "/a/b (2)");
    }
}
