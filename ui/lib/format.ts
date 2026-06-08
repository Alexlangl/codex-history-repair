import type { ProviderDraft, ProviderImportOutcome, RepairOutcome } from "../types";

export function repairLines(repair: RepairOutcome | null) {
  if (!repair) return ["尚未运行"];

  if (noProviderRepairNeeded(repair)) {
    return [
      "结论: 无需修复",
      `当前历史已经在目标 provider (${repair.targetProviderId}) 下。`,
      repair.dryRun
        ? "可以不用点击立即修复。"
        : "没有发现需要迁移的历史。",
      "",
      `目标 provider: ${repair.targetProviderId}`,
      `来源: ${repair.targetProviderSource}`,
      `Index: ${repair.rebuiltSessionIndexEntries} 条`,
      repair.dryRun ? "模式: dry-run" : "模式: 已写入",
    ];
  }

  return [
    `目标 provider: ${repair.targetProviderId}`,
    `来源: ${repair.targetProviderSource}`,
    `JSONL: ${repair.migratedJsonlFiles} 文件 / ${repair.migratedJsonlLines} 行`,
    `SQLite: ${repair.migratedStateRows} 行`,
    `Index: ${repair.rebuiltSessionIndexEntries} 条`,
    `来源 provider: ${repair.sourceProviderIds.join(", ") || "无"}`,
    repair.backupRoot ? `备份: ${repair.backupRoot}` : "备份: 无写入",
    repair.dryRun ? "模式: dry-run" : "模式: 已写入",
    repair.restart ? `重启: ${repair.restart.message}` : "生效方式: 直接修复，无需重启",
  ];
}

export function noProviderRepairNeeded(repair: RepairOutcome) {
  return (
    repair.migratedJsonlFiles === 0 &&
    repair.migratedJsonlLines === 0 &&
    repair.migratedStateRows === 0 &&
    repair.sourceProviderIds.length === 0
  );
}

export function providerPreviewLines(draft: ProviderDraft | null) {
  if (!draft) return ["尚未预览"];

  return [
    `Provider id: ${draft.providerId}`,
    `Name: ${draft.providerName}`,
    `Type: ${draft.importKind}`,
    draft.baseUrl ? `Base URL: ${draft.baseUrl}` : "",
    draft.oauthAccountId ? `OpenAI account: ${draft.oauthAccountId}` : "",
    `API format: ${draft.apiFormat}`,
  ].filter(Boolean);
}

export function providerOutcomeLines(outcome: ProviderImportOutcome | null) {
  if (!outcome) return [];

  return [
    "",
    outcome.message,
    `Backup: ${outcome.backupPath || "none"}`,
  ];
}
