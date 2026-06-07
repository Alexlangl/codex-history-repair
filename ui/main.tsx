import * as Progress from "@radix-ui/react-progress";
import * as Tabs from "@radix-ui/react-tabs";
import * as Toast from "@radix-ui/react-toast";
import * as Tooltip from "@radix-ui/react-tooltip";
import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { ProviderPanel } from "./components/ProviderPanel";
import { RepairPanel } from "./components/RepairPanel";
import { SettingsPanel } from "./components/SettingsPanel";
import { StatusCard } from "./components/StatusCard";
import {
  detectCcSwitch,
  importProvider as importProviderCommand,
  previewProviderImport,
  repairCodexHistory,
  restartCodex,
} from "./lib/api";
import { emptyProviderForm, providerArgs } from "./lib/form";
import type {
  BusyState,
  Detection,
  ProviderDraft,
  ProviderForm,
  ProviderImportOutcome,
  RepairOutcome,
} from "./types";
import "./styles.css";

const root = document.querySelector<HTMLDivElement>("#app");

if (!root) {
  throw new Error("Missing #app root");
}

function App() {
  const [busy, setBusy] = useState<BusyState>("idle");
  const [codexDir, setCodexDir] = useState("");
  const [detection, setDetection] = useState<Detection | null>(null);
  const [draft, setDraft] = useState<ProviderDraft | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [form, setForm] = useState<ProviderForm>(emptyProviderForm);
  const [providerOutcome, setProviderOutcome] =
    useState<ProviderImportOutcome | null>(null);
  const [repair, setRepair] = useState<RepairOutcome | null>(null);
  const [targetProviderId, setTargetProviderId] = useState("");
  const [toast, setToast] = useState("准备就绪");

  useEffect(() => {
    void refreshDetection();
  }, []);

  async function refreshDetection() {
    try {
      const next = await detectCcSwitch();
      setDetection(next);
      setCodexDir((current) => current || next.codexOverrideDir || "");
      setForm((current) => ({
        ...current,
        ccSwitchDir: current.ccSwitchDir || next.ccSwitchDir,
      }));
    } catch (unknownError) {
      setError(String(unknownError));
    }
  }

  async function runRepair(dryRun: boolean) {
    setBusy(dryRun ? "previewing" : "repairing");
    setError(null);
    await nextPaint();
    try {
      const outcome = await repairCodexHistory({
        dryRun,
        restart: false,
        codexDir,
        targetProviderId,
      });
      setRepair(outcome);
      setToast(dryRun ? "预览完成" : "修复完成，未重启 Codex");
    } catch (unknownError) {
      setError(String(unknownError));
    } finally {
      setBusy("idle");
    }
  }

  async function runRestart() {
    setBusy("restarting");
    setError(null);
    await nextPaint();
    try {
      const outcome = await restartCodex();
      setToast(outcome.message);
    } catch (unknownError) {
      setError(String(unknownError));
    } finally {
      setBusy("idle");
    }
  }

  async function previewProvider() {
    setBusy("importing");
    setError(null);
    try {
      const nextDraft = await previewProviderImport(providerArgs(form));
      setDraft(nextDraft);
      setToast("Provider 预览完成");
    } catch (unknownError) {
      setError(String(unknownError));
    } finally {
      setBusy("idle");
    }
  }

  async function importProvider() {
    setBusy("importing");
    setError(null);
    try {
      const outcome = await importProviderCommand(providerArgs(form));
      setProviderOutcome(outcome);
      setToast(outcome.message);
      void refreshDetection();
    } catch (unknownError) {
      setError(String(unknownError));
    } finally {
      setBusy("idle");
    }
  }

  return (
    <Toast.Provider swipeDirection="right">
      <Tooltip.Provider delayDuration={180}>
        <main className="app-shell">
          <aside className="sidebar">
            <div className="brand">
              <div className="brand-icon">CHR</div>
              <div>
                <strong>Codex History Repair</strong>
                <span>cc-switch companion</span>
              </div>
            </div>
            <StatusCard detection={detection} onRefresh={refreshDetection} />
          </aside>

          <section className="workspace">
            <header className="topbar">
              <div>
                <h1>Codex 历史修复</h1>
              </div>
              <span className="badge">{busy === "idle" ? "Ready" : "Working"}</span>
            </header>

            <Progress.Root className="progress-root" value={progressValue(busy)}>
              <Progress.Indicator
                className="progress-indicator"
                style={{ transform: `translateX(-${100 - progressValue(busy)}%)` }}
              />
            </Progress.Root>

            {error ? <div className="error-banner">{error}</div> : null}

            <Tabs.Root className="tabs-root" defaultValue="repair">
              <Tabs.List className="tabs-list" aria-label="Codex History Repair">
                <Tabs.Trigger className="tabs-trigger" value="repair">
                  历史修复
                </Tabs.Trigger>
                <Tabs.Trigger className="tabs-trigger" value="provider">
                  Provider 导入
                </Tabs.Trigger>
                <Tabs.Trigger className="tabs-trigger" value="settings">
                  设置
                </Tabs.Trigger>
              </Tabs.List>
              <Tabs.Content className="tabs-content" value="repair">
                <RepairPanel
                  busy={busy}
                  codexDir={codexDir}
                  repair={repair}
                  targetProviderId={targetProviderId}
                  onCodexDirChange={setCodexDir}
                  onRepair={runRepair}
                  onRestart={runRestart}
                  onTargetProviderChange={setTargetProviderId}
                />
              </Tabs.Content>
              <Tabs.Content className="tabs-content" value="provider">
                <ProviderPanel
                  busy={busy}
                  draft={draft}
                  form={form}
                  outcome={providerOutcome}
                  onChange={setForm}
                  onImport={importProvider}
                  onPreview={previewProvider}
                />
              </Tabs.Content>
              <Tabs.Content className="tabs-content" value="settings">
                <SettingsPanel detection={detection} onRefresh={refreshDetection} />
              </Tabs.Content>
            </Tabs.Root>
          </section>
        </main>
      </Tooltip.Provider>
      <Toast.Root
        className="toast-root"
        onOpenChange={() => setToast("")}
        open={Boolean(toast)}
      >
        <Toast.Title className="toast-title">{toast}</Toast.Title>
      </Toast.Root>
      <Toast.Viewport className="toast-viewport" />
    </Toast.Provider>
  );
}

function progressValue(busy: BusyState) {
  if (busy === "idle") return 0;
  if (busy === "repairing") return 72;
  if (busy === "restarting") return 72;
  return 46;
}

async function nextPaint() {
  await new Promise<void>((resolve) => {
    window.requestAnimationFrame(() => resolve());
  });
}

createRoot(root).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
