# `ccs list` 展示与本地检查方案

## 结论

查询与检查功能合并到一个公开的 `list` 子命令更符合使用场景；现有 `edit` 命令继续保留。`list` 默认回答“有哪些可选配置”，`--verbose` 再回答“每项配置了什么、当前本地检查是否通过”。原 `validate` 的检查能力并入 `list --verbose`，不再作为独立子命令。

一条可选项建议称为 **Profile（运行配置）**，而 `provider` 专指 DeepSeek、OpenAI 这样的服务提供商。每条 `[[profiles]]` 还包含 agent 工具、模型、环境变量和启动参数，叫整个对象 `Provider` 会与其中的 `provider` 字段重名且语义不准。命令参数直接改为 `-p, --profile <PROFILE_ID>`；不保留 `--provider` 别名；旧写法给出改用 `--profile` 的错误提示。新配置使用 `[[profiles]]` / `[profiles.env]`，旧 `[[providers]]` / `[providers.env]` 继续可读；`provider` 字段与既有 ID 保持不变。内部类型使用 `Profile`，具体迁移与回滚规则见 [Profile 命名与配置键](profile-naming.md)。

## 0.2.10 问题依据

- `Config.providers` 的每一项同时有 `id`、`provider`、`model`、`executable`、环境变量和启动参数（`src/main.rs:82-105`）。`provider` 字段注释明确表示服务商；当前 `list` 却把整条记录叫 provider，且默认同时输出表格与所有环境变量（`src/main.rs:701-753`）。
- 当前表格为 `ID/TOOL/PROVIDER/MODEL/RESUME`；列宽使用 UTF-8 字节数计算，中文提供商名称与英文名称混排时会错位（`src/main.rs:200-213,701-729`）。
- 默认遮盖仅靠环境变量名包含四个关键词；隔离配置中的 `API_CREDENTIAL` 会在标注为“masked”的输出中原样显示（`src/main.rs:22,593-604`）。`dry-run` 复用同一逻辑。
- 当前 `validate` 通过外部 `which` 检查父进程 `PATH`，而实际启动注入了每条配置的环境变量（`src/main.rs:755-772,685-695`）。隔离验证已复现“可启动却验证失败”和“验证成功却启动失败”。重复 ID、空字段及拼错的 `suports_resume` 也能通过现有检查。现有 41 项单元测试通过，但未覆盖上述命令级场景。

## 命令与输出设计

### 默认 `ccs list`

按配置顺序只显示四列：**ID、PROVIDER、MODEL、AGENT**。`AGENT` 使用面向人的名称 `Claude Code` / `Codex`；可执行文件名和路径留在详细信息中。默认列表不展开环境变量、不检查 PATH，也不增加 `RESUME` 和状态列。TOML 无法解析或存在重复 ID 等结构错误时，输出明确错误并返回非零；正常展示返回 0。首次没有配置文件时继续生成现有模板，并明确提示这是待填写的示例配置。

```text
ID          PROVIDER  MODEL                AGENT
─────────────────────────────────────────────────────────
deepseek    DeepSeek  deepseek-v4-pro      Claude Code
codex       OpenAI    gpt-4o               Codex
```

用终端显示宽度对齐列，而非字符串字节数；颜色仅在交互终端且未设置 `NO_COLOR` 时使用，状态也始终有文字。窄终端改为逐项多行展示，保留完整 ID 与模型名；重定向输出不带 ANSI 控制码。保持配置顺序，不按服务商或状态重排。

### `ccs list --verbose`

先显示同一张四列表，再按 ID 展开每条配置：agent 可执行文件名及查找路径、环境变量名与默认遮盖的值、本地检查结果及失败原因。`--show-secrets` 为显式展示原始环境变量值的开关，支持 `ccs list --verbose --show-secrets`；`--verbose` 单独使用时，**所有**环境变量值显示为固定的 `***masked***`，不按变量名猜测敏感性，也不输出原值长度。共用的 `dry-run` 遮盖逻辑同步修正。

```text
deepseek  OK — local checks passed
  Agent   claude  /opt/homebrew/bin/claude
  Env     ANTHROPIC_BASE_URL    = ***masked***
          ANTHROPIC_AUTH_TOKEN  = ***masked***

codex     FAIL — codex not found in effective PATH
  Agent   codex
  Env     OPENAI_API_KEY = ***masked***
```

`--verbose` 汇总全部条目；任一条本地检查失败返回 1，全部通过返回 0。检查结果只代表配置结构与当前环境下的可执行文件定位，不代表凭据、模型或远端服务可用。原 `ccs validate` 的调用方迁移到 `ccs list --verbose`。对于日志和脚本，`OK`/`FAIL` 与原因是实际文本，不单靠颜色或符号传意。

### 本地检查规则

1. 共用的配置加载路径检查 Profile 数组非空、`id`/`provider`/`model` 去除首尾空白后非空，以及 `id` 全局唯一。保留原始值，不自动修剪或重命名。重复 ID 会让 `-p ID` 选中首项，也会混淆 recent，因此所有入口一致拒绝。
2. 详细模式额外检查顶层与 `[[profiles]]` 的未知字段，报出所在项和字段名；`[profiles.env]` 的任意变量名合法。普通启动和默认 `list` 暂不因未知字段失败，以兼容已有配置。必填字段缺失及类型错误继续由 TOML/Serde 解析报错。
3. 详细模式按每项实际生效的 `PATH` 顺序定位 `claude`/`codex`，包含 `profiles.env.PATH` 的覆盖；不再调用外部 `which`。确认目标是可执行文件且有执行权限，允许有效符号链接；不运行 agent 程序。实现与测试覆盖空目录项、相对目录及未设置 `PATH` 的 Unix 行为。
4. 首次没有配置文件时沿用现有的模板创建流程，并在输出中明确提示模板尚需填写；详细模式的成功仍只代表本地结构与可执行文件检查，不代表占位符凭据可用。

## 契约变更与影响

| 契约 | 变更必要性与前后行为 | 影响及消费方动作 |
|---|---|---|
| CLI 命令 | 移除公开的 `validate`；`list` 默认仅四列，`list --verbose` 输出本地检查及环境变量；将长参数 `--provider` 改为 `--profile`，短参数 `-p` 不变。 | 使用 `ccs validate` 的脚本改为 `ccs list --verbose`，以退出码判断本地检查。`-p` 保持可用；使用 `--provider` 的调用方改为 `--profile`。 |
| 展示与保密 | 详细列表和 `dry-run` 默认遮盖所有环境变量值；`--show-secrets` 才输出原值。 | 原先默认可见的非关键词变量值会被遮盖；确需原值的调用方显式加参数。表格列和行布局变化，依赖固定文本列的脚本需调整。 |
| 配置语义 | 空关键字段或重复 ID 从可解析变为错误；未知顶层/Profile 字段在详细模式报错，env 键保持开放。 | 异常存量配置需手工更正。合法 `[[providers]]` 配置、短参数 `-p`、agent 启动参数和 recent 文件不迁移。 |
| 检查语义 | 从外部 `which` 与父进程 `PATH` 改为有效 `PATH` 的文件检查。 | 检查结果与退出码可能变化，尤其是自定义 `PATH` 或缺少 `which` 的环境。首次创建的模板仍需填写，检查成功不表示线上服务可用。 |
| 命名 | 界面和文档用 Profile 表示整条运行配置，`provider` 保留为服务商字段。 | 配置文件名与现有 `-p` 保持兼容；推荐 TOML 键改为 `profiles`，兼容读取旧 `providers`；`--provider` 需要迁移为 `--profile`，无配置自动改写。 |

recent 持久化格式、启动命令构造规则均无变化；配置表头的新旧格式兼容规则见 Profile 命名方案。**DDL：无变化。**

## 上线步骤

1. 发布前检查存量配置中的重复 ID、空字段和未知顶层/Profile 字段；记录需要人工修正的项，不自动改写用户文件。更新 CLI 帮助、README 和 CHANGELOG，明确 `validate` 到 `list --verbose`、`--provider` 到 `--profile` 的迁移及成功语义。
2. 固定 Case 验收：四列表的顺序、中文/英文显示宽度、窄终端及重定向输出；详细模式环境变量全部遮盖与显式展示；每项检查结果及汇总退出码；首次缺配置生成模板及提示、坏 TOML、重复 ID、未知字段；无 `which` 但 agent CLI 存在；Profile 覆盖 `PATH` 的成功与失败场景。
3. 独立验证旧功能：有效旧配置下的直接 `-p` 与新 `--profile` 启动，确认旧 `--provider` 按新契约拒绝；另验证交互菜单、resume、参数透传、recent 与 `dry-run`；运行 `cargo fmt --check`、`cargo clippy -- -D warnings`、`cargo test`、`cargo build`，并验证 macOS 与 Linux 的 PATH 查找。
4. 发布单个 CLI 二进制。无数据库迁移、重算或自动任务恢复。验收旧配置与新二进制共存、旧二进制回滚读取同一配置；发布后关注 `list --verbose` 失败原因及用户反馈。
5. 回滚旧版二进制时，已迁移为 `profiles` 的配置须同步恢复旧表头或配置备份；recent 数据兼容。回滚后 `validate` 命令恢复，旧版默认输出仍可能显示未被关键词命中的环境变量值，PATH 检查也恢复旧行为。

## 已确定的取舍

- 查询和检查功能统一到 `list`，已有 `validate` 调用迁移到 `list --verbose`。旧命令仅返回迁移错误，不执行检查，帮助中不再展示。
- 整条可启动配置称为 Profile；`provider` 只表示服务商，新配置键使用 `profiles`、旧 `providers` 键兼容读取，`-p` 保留，`--provider` 直接更名为 `--profile`。

## 开发验证

- 本地通过 41 项单元测试及 10 项隔离 CLI 集成测试；`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo audit` 和 release 构建通过。
- 集成测试覆盖默认四列、环境变量全部遮盖、显式展示、未知配置项、重复 ID、实际 PATH 覆盖、无 `which`、执行权限、相对路径、符号链接、旧命令迁移提示、启动参数透传和 recent。
- 独立 PTY 走查覆盖 100 列与 36 列布局、中文显示宽度、颜色与 `NO_COLOR`、菜单键盘选择、resume、Recent 默认选择及 Esc 取消。
- GitHub CI 的 macOS job 增加集成测试执行，与 Linux job 一起验证 Unix 平台差异。发布版本为 `0.3.0`，体现 CLI 契约变化。
- CI 审计工具固定为 `cargo-audit 0.22.2 --locked`。首次 CI 失败发生在未锁定依赖安装阶段：`kstring 2.0.5` 要求 Rust 1.96，发布锁文件中的 2.0.2 与项目 1.95 工具链兼容。
