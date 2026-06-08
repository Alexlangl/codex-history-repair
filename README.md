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

执行修复：

```bash
codex-history-repair repair
```

通常不需要重启 Codex。如果仍想在修复后重启：

```bash
codex-history-repair repair --restart
```

指定 Codex 目录：

```bash
codex-history-repair repair --codex-dir ~/.codex --dry-run
```

指定要修复到哪个 provider 桶：

```bash
codex-history-repair repair --target-provider openai
codex-history-repair repair --target-provider custom
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

macOS 如果提示 App“已损坏，无法打开”，先把 App 移到 `Applications`，然后执行：

```bash
sudo xattr -rd com.apple.quarantine "/Applications/Codex History Repair.app"
```

这是 macOS 对未公证下载 App 的拦截，不是历史修复数据损坏。

## Provider 导入

支持的 JSON 格式：

- CPA Codex 账号 JSON。
- sub2api 导出的 `accounts[]` OpenAI OAuth 账号。
- cockpit 导出的 Codex 账号 JSON。

CPA / cockpit 格式需要包含 OpenAI 账号 token 信息，尤其是 `access_token`、`refresh_token` 和 `id_token`：

```json
{
  "type": "codex",
  "email": "mark@example.com",
  "name": "mark@example.com",
  "account_id": "00000000-0000-4000-9000-000000000000",
  "chatgpt_account_id": "00000000-0000-4000-9000-000000000000",
  "plan_type": "plus",
  "id_token": "paste-real-id-token-here",
  "access_token": "paste-real-access-token-here",
  "refresh_token": "paste-real-refresh-token-here",
  "session_token": "paste-real-session-token-here",
  "last_refresh": "2026-06-06T11:14:06.884Z",
  "expired": "2026-08-06T14:29:36.155Z"
}
```

sub2api 格式会读取 `accounts[]` 里的 OpenAI OAuth 账号：

```json
{
  "accounts": [
    {
      "name": "mark@example.com",
      "platform": "openai",
      "type": "oauth",
      "credentials": {
        "access_token": "paste-real-access-token-here",
        "refresh_token": "paste-real-refresh-token-here",
        "id_token": "paste-real-id-token-here",
        "account_id": "00000000-0000-4000-9000-000000000000",
        "chatgpt_account_id": "00000000-0000-4000-9000-000000000000",
        "workspace_id": "00000000-0000-4000-9000-000000000000",
        "expires_in": 5282129,
        "email": "mark@example.com",
        "plan_type": "plus"
      },
      "extra": {
        "email": "mark@example.com",
        "last_refresh": "2026-06-06T11:14:06.884Z",
        "account_id": "00000000-0000-4000-9000-000000000000",
        "chatgpt_account_id": "00000000-0000-4000-9000-000000000000"
      }
    }
  ]
}
```

Provider 导入只写入 cc-switch 的 Codex provider 数据库。它不会自动切换当前 provider，不会修改正在使用的 Codex 配置，也不会重启 Codex。

不支持手动创建 `base_url + API key` provider。

## 安全说明

- `--dry-run` 不会写入文件。
- 修复历史前会先备份 Codex 历史文件。
- 导入 provider 前会先备份 `cc-switch.db`。
- Provider 导入不会自动切换 cc-switch 当前 provider。
