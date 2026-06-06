# codex-history-repair

English | [中文](./README.md)

`codex-history-repair` repairs local Codex conversation history after switching Codex providers with cc-switch or similar tools.

It provides:

- A CLI.
- A desktop Tauri App.

## What It Does

- Repairs Codex history files so sessions can appear under the current provider bucket.
- Syncs `openai` sessions into `custom` when the current target provider is `custom`.
- Syncs `custom` sessions into `openai` when the current target provider is `openai`.
- Migrates known cc-switch legacy Codex provider buckets into the current target provider.
- Rebuilds `session_index.jsonl` when needed.
- Creates backups before writing.
- Imports OpenAI account JSON into cc-switch Codex providers.

It updates local Codex files such as:

- `~/.codex/sessions/**/*.jsonl`
- `~/.codex/archived_sessions/**/*.jsonl`
- `~/.codex/state_5.sqlite`
- `~/.codex/session_index.jsonl`

## CLI Usage

Install the CLI first:

```bash
cargo install --git https://github.com/Alexlangl/codex-history-repair --package codex-history-repair
```

If you already cloned the source code, you can also run it from the project directory:

```bash
cargo run -p codex-history-repair -- repair --dry-run
```

Preview repair changes without writing files:

```bash
codex-history-repair repair --dry-run
```

Repair history and restart Codex:

```bash
codex-history-repair repair --restart
```

Repair with an explicit Codex directory:

```bash
codex-history-repair repair --codex-dir ~/.codex --dry-run
```

Repair into an explicit provider bucket:

```bash
codex-history-repair repair --target-provider openai --restart
codex-history-repair repair --target-provider custom --restart
```

Preview OpenAI account JSON import:

```bash
codex-history-repair import-provider --from-json token.json --preview
```

Import OpenAI account JSON into cc-switch:

```bash
codex-history-repair import-provider --from-json token.json
```

## App Usage

Start the desktop app from source:

```bash
pnpm install
pnpm dev
```

In the app:

- Use `历史修复` to preview or repair Codex history.
- Use `Provider 导入` to import OpenAI account JSON into cc-switch.
- Use `设置` to check detected cc-switch and Codex paths.

If macOS says the app is "damaged" and cannot be opened, move it to `Applications` first, then run:

```bash
sudo xattr -rd com.apple.quarantine "/Applications/Codex History Repair.app"
```

This is macOS blocking a downloaded app that has not been notarized. It does not mean your Codex history data is damaged.

## Provider Import

Supported JSON formats:

- CPA Codex account JSON.
- sub2api exports with OpenAI OAuth accounts in `accounts[]`.
- cockpit Codex account JSON.

CPA / cockpit JSON must include the OpenAI account `access_token` and `id_token`:

```json
{
  "type": "codex",
  "email": "mark@example.com",
  "access_token": "paste-real-access-token-here",
  "id_token": "paste-real-id-token-here",
  "account_id": "00000000-0000-4000-9000-000000000000"
}
```

sub2api JSON is read from OpenAI OAuth accounts in `accounts[]`:

```json
{
  "accounts": [
    {
      "name": "mark@example.com",
      "platform": "openai",
      "type": "oauth",
      "credentials": {
        "access_token": "paste-real-access-token-here",
        "id_token": "paste-real-id-token-here",
        "account_id": "00000000-0000-4000-9000-000000000000"
      }
    }
  ]
}
```

Provider import only writes to cc-switch's Codex provider database. It does not switch the active provider, does not modify live Codex config, and does not restart Codex.

Manual `base_url + API key` provider creation is not supported.

## Safety

- `--dry-run` does not write files.
- Repair creates backups before modifying Codex history.
- Provider import backs up `cc-switch.db` before writing.
- Provider import never switches the current cc-switch provider automatically.
