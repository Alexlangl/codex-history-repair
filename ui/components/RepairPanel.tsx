import * as ScrollArea from "@radix-ui/react-scroll-area";
import { repairLines } from "../lib/format";
import type { BusyState, RepairOutcome } from "../types";
import { ConfirmRepairDialog } from "./ConfirmRepairDialog";
import { Button, Field, Panel } from "./ui";

export function RepairPanel({
  busy,
  codexDir,
  repair,
  targetProviderId,
  onCodexDirChange,
  onTargetProviderChange,
  onRepair,
}: {
  busy: BusyState;
  codexDir: string;
  repair: RepairOutcome | null;
  targetProviderId: string;
  onCodexDirChange: (value: string) => void;
  onTargetProviderChange: (value: string) => void;
  onRepair: (dryRun: boolean) => void;
}) {
  return (
    <div className="content-grid">
      <Panel title="历史修复" eyebrow="Repair">
        <Field label="Codex 目录">
          <input
            value={codexDir}
            onChange={(event) => onCodexDirChange(event.target.value)}
            placeholder="留空自动使用 ~/.codex 或 cc-switch override"
          />
        </Field>
        <Field label="目标 provider id">
          <input
            value={targetProviderId}
            onChange={(event) => onTargetProviderChange(event.target.value)}
            placeholder="留空自动检测"
          />
        </Field>
        <div className="actions">
          <Button
            disabled={busy !== "idle"}
            onClick={() => onRepair(true)}
            variant="secondary"
          >
            预览影响范围
          </Button>
          <ConfirmRepairDialog
            disabled={busy !== "idle"}
            isRepairing={busy === "repairing"}
            onConfirm={() => onRepair(false)}
          />
        </div>
        <RepairProgress busy={busy} />
      </Panel>
      <ResultPanel lines={repairLines(repair)} title="修复结果" />
    </div>
  );
}

function RepairProgress({ busy }: { busy: BusyState }) {
  if (busy !== "previewing" && busy !== "repairing") {
    return null;
  }

  const isRepairing = busy === "repairing";
  const title = isRepairing ? "正在修复历史" : "正在预览影响范围";
  const note = isRepairing
    ? "正在备份并写入 Codex 历史，完成后会刷新结果。"
    : "正在扫描 Codex 历史和状态数据库，完成后会刷新结果。";

  return (
    <div aria-live="polite" className="repair-progress" role="status">
      <div className="repair-progress-header">
        <strong>{title}</strong>
        <span>{isRepairing ? "Repairing" : "Previewing"}</span>
      </div>
      <div aria-hidden="true" className="repair-progress-track">
        <div className="repair-progress-fill" />
      </div>
      <p>{note}</p>
    </div>
  );
}

export function ResultPanel({
  lines,
  title,
}: {
  lines: string[];
  title: string;
}) {
  return (
    <Panel title={title} eyebrow="Result">
      <ScrollArea.Root className="scroll-root">
        <ScrollArea.Viewport className="scroll-viewport">
          <pre>{lines.join("\n")}</pre>
        </ScrollArea.Viewport>
        <ScrollArea.Scrollbar className="scrollbar" orientation="vertical">
          <ScrollArea.Thumb className="scroll-thumb" />
        </ScrollArea.Scrollbar>
      </ScrollArea.Root>
    </Panel>
  );
}
