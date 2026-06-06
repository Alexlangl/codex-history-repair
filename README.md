# codex-history-repair

中文 | [English](./README.en.md)

`codex-history-repair` 用来修复通过 cc-switch 或类似工具切换 Codex provider 后，本地 Codex 历史会话看不到的问题。

它提供：

- CLI 命令行。
- Tauri 桌面 App。

## 它做什么

- 修复 Codex 本地历史文件，让会话能出现在当前 provider 桶里。
- 当前目标 provider 是 `custom` 时，会把 `openai` 桶里的会话同步到 `custom`。
- 当前目标 provider 是 `openai` 时，会把 `custom` 桶里的会话同步到 `openai`。
- 会把 cc-switch 已知的旧 Codex provider 桶迁移到当前目标 provider。
- 必要时重建 `session_index.jsonl`。
- 写入前会先创建备份。
- 支持把 OpenAI 账号 JSON 导入到 cc-switch 的 Codex provider。

它会处理这些本地 Codex 文件：

- `~/.codex/sessions/**/*.jsonl`
- `~/.codex/archived_sessions/**/*.jsonl`
- `~/.codex/state_5.sqlite`
- `~/.codex/session_index.jsonl`

## CLI 用法

先安装 CLI：

```bash
cargo install --git https://github.com/Alexlangl/codex-history-repair --package codex-history-repair
```

如果已经下载了源码，也可以在项目目录里直接运行：

```bash
cargo run -p codex-history-repair -- repair --dry-run
```

只预览修复影响，不写文件：

```bash
codex-history-repair repair --dry-run
```

执行修复并重启 Codex：

```bash
codex-history-repair repair --restart
```

指定 Codex 目录：

```bash
codex-history-repair repair --codex-dir ~/.codex --dry-run
```

指定要修复到哪个 provider 桶：

```bash
codex-history-repair repair --target-provider openai --restart
codex-history-repair repair --target-provider custom --restart
```

预览导入 OpenAI 账号 JSON：

```bash
codex-history-repair import-provider --from-json token.json --preview
```

导入 OpenAI 账号 JSON 到 cc-switch：

```bash
codex-history-repair import-provider --from-json token.json
```

## App 用法

从源码启动桌面 App：

```bash
pnpm install
pnpm dev
```

在 App 里：

- 使用 `历史修复` 预览或修复 Codex 历史。
- 使用 `Provider 导入` 把 OpenAI 账号 JSON 导入 cc-switch。
- 使用 `设置` 查看检测到的 cc-switch 和 Codex 路径。

## Provider 导入

支持的 JSON 格式：

- CPA Codex 账号 JSON。
- sub2api 导出的 `accounts[]` OpenAI OAuth 账号。

Provider 导入只写入 cc-switch 的 Codex provider 数据库。它不会自动切换当前 provider，不会修改正在使用的 Codex 配置，也不会重启 Codex。

不支持手动创建 `base_url + API key` provider。

## 安全说明

- `--dry-run` 不会写入文件。
- 修复历史前会先备份 Codex 历史文件。
- 导入 provider 前会先备份 `cc-switch.db`。
- Provider 导入不会自动切换 cc-switch 当前 provider。
