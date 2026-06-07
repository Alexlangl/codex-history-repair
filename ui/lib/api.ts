import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import type {
  Detection,
  ProviderDraft,
  ProviderImportOutcome,
  RepairOutcome,
  RestartOutcome,
} from "../types";

const isTauri = "__TAURI_INTERNALS__" in window;

export async function detectCcSwitch() {
  if (isTauri) {
    return tauriInvoke<Detection>("detect_cc_switch");
  }

  return {
    ccSwitchDir: "~/.cc-switch",
    dbPath: "~/.cc-switch/cc-switch.db",
    settingsPath: "~/.cc-switch/settings.json",
    dbExists: false,
    settingsExists: false,
    codexOverrideDir: null,
    currentCodexProviderId: null,
    currentCodexProviderName: null,
  } satisfies Detection;
}

export async function repairCodexHistory(args: Record<string, unknown>) {
  if (!isTauri) {
    throw new Error("请在 Tauri App 中运行修复操作");
  }
  return tauriInvoke<RepairOutcome>("repair_codex_history", args);
}

export async function restartCodex() {
  if (!isTauri) {
    throw new Error("请在 Tauri App 中重启 Codex");
  }
  return tauriInvoke<RestartOutcome>("restart_codex_app");
}

export async function previewProviderImport(args: Record<string, unknown>) {
  if (!isTauri) {
    throw new Error("请在 Tauri App 中预览 provider 导入");
  }
  return tauriInvoke<ProviderDraft>("preview_provider_import", args);
}

export async function importProvider(args: Record<string, unknown>) {
  if (!isTauri) {
    throw new Error("请在 Tauri App 中写入 cc-switch");
  }
  return tauriInvoke<ProviderImportOutcome>("import_provider", args);
}
