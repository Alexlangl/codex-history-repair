use codex_history_repair_core::{
    default_cc_switch_dir, detect_cc_switch as detect_cc_switch_core, import_cc_switch_provider,
    parse_provider_import_draft, repair_codex_history as repair_history, resolve_default_codex_dir,
    restart_codex, CcSwitchDetection, ProviderImportDraft, ProviderImportOptions,
    ProviderImportOutcome, RepairOptions,
};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UiRepairOutcome {
    #[serde(flatten)]
    repair: codex_history_repair_core::RepairOutcome,
    restart: Option<codex_history_repair_core::RestartOutcome>,
}

#[tauri::command]
fn detect_cc_switch() -> CcSwitchDetection {
    detect_cc_switch_core()
}

#[tauri::command]
async fn repair_codex_history(
    dry_run: bool,
    restart: bool,
    codex_dir: Option<String>,
    target_provider_id: Option<String>,
) -> Result<UiRepairOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repair = repair_history(RepairOptions {
            codex_dir: codex_dir
                .filter(|value| !value.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(resolve_default_codex_dir),
            target_provider_id,
            dry_run,
        })
        .map_err(|error| error.to_string())?;

        let restart = if restart && !dry_run {
            Some(restart_codex().map_err(|error| error.to_string())?)
        } else {
            None
        };

        Ok(UiRepairOutcome { repair, restart })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn preview_provider_import(
    cc_switch_dir: Option<String>,
    provider_id: Option<String>,
    provider_name: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    api_format: Option<String>,
    model: Option<String>,
    json_text: Option<String>,
) -> Result<ProviderImportDraft, String> {
    parse_provider_import_draft(&provider_options(
        cc_switch_dir,
        provider_id,
        provider_name,
        base_url,
        api_key,
        api_format,
        model,
        json_text,
    ))
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn import_provider(
    cc_switch_dir: Option<String>,
    provider_id: Option<String>,
    provider_name: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    api_format: Option<String>,
    model: Option<String>,
    json_text: Option<String>,
) -> Result<ProviderImportOutcome, String> {
    import_cc_switch_provider(provider_options(
        cc_switch_dir,
        provider_id,
        provider_name,
        base_url,
        api_key,
        api_format,
        model,
        json_text,
    ))
    .map_err(|error| error.to_string())
}

fn provider_options(
    cc_switch_dir: Option<String>,
    provider_id: Option<String>,
    provider_name: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    api_format: Option<String>,
    model: Option<String>,
    json_text: Option<String>,
) -> ProviderImportOptions {
    ProviderImportOptions {
        cc_switch_dir: cc_switch_dir
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(default_cc_switch_dir),
        provider_id,
        provider_name,
        base_url,
        api_key,
        api_format,
        model,
        json_text,
    }
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            detect_cc_switch,
            repair_codex_history,
            preview_provider_import,
            import_provider
        ])
        .run(tauri::generate_context!())
        .expect("error while running Codex History Repair");
}
