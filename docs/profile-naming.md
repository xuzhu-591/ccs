# Profile 命名统一

## 目标与范围

整条启动配置统一称为 Profile，provider 只表示其中的服务商。清理 0.3.0 遗留的菜单类型、函数、变量、测试名称及贡献指南；内置模板改名为 `src/default_profiles.toml`，同步 README 和构建引用。

## 契约变更与影响

| 项目 | 实现与影响 |
|---|---|
| 内部配置结构 | 集合改为 `Config.profiles`，使用 `#[serde(rename = "providers")]` 读取现有 TOML 键，避免内部继续混用服务商与运行配置。 |
| 配置文件 | 保留 `[[providers]]`、`[providers.env]` 和服务商字段 `provider`；不引入 `[[profiles]]` 别名，不改写用户配置、已有 ID 或 recent。 |
| 菜单与测试 | 类型、函数、参数和局部变量统一使用 Profile；选择、Recent、resume、参数透传和显示算法不变。 |
| 模板 | 仅源码文件名变化，模板内容和首次生成的 `config.toml` 格式保持不变。 |
| 对外命令与诊断 | 沿用 0.3.0 的 `--profile` 与 `list --verbose`；保留旧 `--provider` 的迁移提示，以及指向实际配置键的 `providers[index]` 错误路径。 |

数据库、持久化数据、算法及协议无变化。**DDL：无变化。** 表格中的 `PROVIDER` 仍表示服务商。历史 CHANGELOG 和 0.2.10 问题依据保留对应版本的原始命名。

## 验证

运行现有单元测试和隔离 CLI 集成测试，覆盖旧 TOML 的解析、校验、启动、环境变量注入、参数透传及 recent。运行 fmt、clippy、release 构建和打包验证，检查模板重命名后仍被包含。对发布安装后的二进制再次运行 CLI 测试及 PTY 菜单走查，核对本机配置与 recent 校验和。

## 上线步骤

1. 发布前通过上述检查，复核剩余 provider 引用仅表示服务商、兼容配置键、旧参数迁移或历史记录。
2. 发布补丁版 0.3.1，合并 PR 后按现有标签 CI 发布 crates.io 与 GitHub Release，再安装正式版。
3. 验收旧配置直接运行 `list`、`list --verbose` 和菜单；新旧二进制可读取相同配置，无迁移、重算或自动任务恢复。
4. 回滚时替换为 0.3.0 二进制即可，配置和 recent 无需处理。
