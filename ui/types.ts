export type BusyState =
  | "idle"
  | "previewing"
  | "repairing"
  | "restarting"
  | "importing";

export interface Detection {
  ccSwitchDir: string;
  dbPath: string;
  settingsPath: string;
  dbExists: boolean;
  settingsExists: boolean;
  codexOverrideDir?: string | null;
  currentCodexProviderId?: string | null;
  currentCodexProviderName?: string | null;
}

export interface RestartOutcome {
  killedProcesses: number;
  launched: boolean;
  message: string;
}

export interface RepairOutcome {
  codexDir: string;
  targetProviderId: string;
  targetProviderSource: string;
  sourceProviderIds: string[];
  scannedJsonlFiles: number;
  migratedJsonlFiles: number;
  migratedJsonlLines: number;
  migratedStateRows: number;
  rebuiltSessionIndexEntries: number;
  backupRoot?: string | null;
  dryRun: boolean;
  skippedReason?: string | null;
  restart?: RestartOutcome | null;
}

export interface ProviderDraft {
  providerId: string;
  providerName: string;
  importKind: string;
  baseUrl?: string | null;
  apiKey?: string | null;
  oauthAccountId?: string | null;
  apiFormat: string;
  model: string;
}

export interface ProviderImportOutcome {
  providerId: string;
  providerName: string;
  importKind: string;
  baseUrl?: string | null;
  oauthAccountId?: string | null;
  apiFormat: string;
  created: boolean;
  updated: boolean;
  backupPath?: string | null;
  oauthStorePath?: string | null;
  message: string;
}

export interface ProviderForm {
  ccSwitchDir: string;
  jsonText: string;
  providerId: string;
  providerName: string;
}
