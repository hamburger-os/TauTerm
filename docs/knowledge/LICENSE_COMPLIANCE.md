# 第三方许可证与分发知识索引

> 本文是工程审查清单，不构成法律意见。具体商业分发方式发生变化时，应进行相应法律/合规复核。

## TauTerm 自有代码

TauTerm 自有代码使用 **MIT OR Apache-2.0**。

- MIT: 根目录 `LICENSE`
- Apache-2.0: 根目录 `LICENSE-APACHE`
- SPDX 标识: `MIT OR Apache-2.0`

`package.json` 与 `src-tauri/Cargo.toml` 必须保持相同许可证标识。

## GPL-2.0-or-later / com0com

- com0com 官方项目: https://sourceforge.net/projects/com0com/
- 3.0.0.0 官方文件目录: https://sourceforge.net/projects/com0com/files/com0com/3.0.0.0/
- GPL version 2 官方文本: https://www.gnu.org/licenses/old-licenses/gpl-2.0.html

TauTerm Windows 安装包再分发 com0com 的独立 driver/setup 二进制，因此必须保留许可证、上游来源和对应源代码获取信息。GPL version 2 对 object/executable distribution 的源码提供方式有具体要求；不能把“README 有一个上游链接”自动等同于所有分发场景都已满足义务。

仓库分发材料：

- `resources/com0com/COPYING-GPL-2.0.txt`
- `resources/com0com/SOURCE.md`
- `resources/com0com/README.md`

## MPL-2.0 / TCNOpen

- MPL 2.0: https://www.mozilla.org/MPL/2.0/
- Mozilla MPL FAQ: https://www.mozilla.org/MPL/2.0/FAQ/
- TCNOpen: https://sourceforge.net/projects/tcnopen/files/TRDP/

TauTerm vendored TCNOpen 源码并编译进 TRDP native sidecar。MPL 是 file-level copyleft；被修改的 covered source 和许可证/notice 必须保持可获得。

TauTerm 的实际 vendored tree包含少量 downstream patch，不能描述成“完全未修改上游”。

仓库材料：

- `src-tauri/vendor/tcnopen/LICENSE`
- `src-tauri/vendor/tcnopen/SOURCE.json`
- `src-tauri/vendor/tcnopen/README.md`

## MIT OR Apache-2.0 / riperf3

TauTerm 使用修改过的 vendored `riperf3 0.8.0`。

- Upstream: https://github.com/therealevanhenry/riperf3
- 本地 license: `src-tauri/vendor/riperf3/LICENSE-MIT.txt`、`LICENSE-APACHE.txt`
- 修改说明: `src-tauri/vendor/riperf3/VENDOR-NOTES.md`

Apache 2.0 要求修改文件保留明显修改声明等条件；TauTerm 通过 vendored fork 的修改记录保持可追溯性，但新增 patch 时仍应检查具体文件 notice 要求。

## Lua 5.4

- Lua license: https://www.lua.org/license.html
- Lua 5.4 manual: https://www.lua.org/manual/5.4/

TauTerm 通过 `mlua` 的 vendored Lua 5.4 构建链嵌入 Lua runtime，因此二进制分发 notice 应保留 Lua 的 MIT 许可/版权声明。根 `THIRD_PARTY_LICENSES.md` 保存该 notice。

## Npcap / libpcap

TauTerm 当前不再分发 Npcap 或 libpcap：

- Windows live capture 使用用户已安装的 Npcap；
- Linux/macOS live capture使用系统 libpcap。

如果未来把它们打进安装包，必须在合并该变更前重新审查许可证与再分发条件。

## 依赖审查规则

普通 npm/Cargo 依赖的版本由 lockfile 追踪。出现下面任一情况时，应提升到根第三方清单：

- 将源码复制到仓库；
- 对依赖打 patch/fork；
- 单独捆绑二进制/驱动；
- 引入 GPL/MPL/LGPL 等需要额外分发义务的组件；
- 嵌入需要明确保留 notice 的 runtime；
- 许可证或来源无法由正常 package metadata 清楚追踪。

CI 的 `npm run license:check` 只做可机械验证的完整性检查，不能替代上述人工判断。
