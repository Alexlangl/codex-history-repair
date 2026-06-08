use chrono::{Local, Utc};
use rusqlite::{backup::Backup, params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;
use sysinfo::{ProcessesToUpdate, System};
use thiserror::Error;
use toml_edit::DocumentMut;
use walkdir::WalkDir;

const CODEX_STATE_DB_FILENAME: &str = "state_5.sqlite";
const CODEX_DEFAULT_MODEL_PROVIDER_ID: &str = "openai";
const CODEX_LOCAL_ACCESS_PROVIDER_ID: &str = "codex_local_access";
const CC_SWITCH_DB_FILENAME: &str = "cc-switch.db";
const DEFAULT_CODEX_MODEL: &str = "gpt-5-codex";

// Keep this list aligned with cc-switch's Codex legacy migration allowlist.
// Never include `openai` or `custom`: those are valid current provider buckets
// and rewriting them can hide active/official Codex sessions.
const LEGACY_PROVIDER_IDS: &[&str] = &[
    "ccswitch",
    "aicodemirror",
    "aicoding",
    "aigocode",
    "aihubmix",
    "ark_agentplan",
    "bailian",
    "bailing",
    "byteplus",
    "claudecn",
    "compshare",
    "compshare_coding",
    "crazyrouter",
    "ctok",
    "cubence",
    "deepseek",
    "dmxapi",
    "doubaoseed",
    "eflowcode",
    "kimi",
    "lemondata",
    "longcat",
    "micu",
    "minimax",
    "minimax_en",
    "modelscope",
    "novita",
    "nvidia",
    "openrouter",
    "packycode",
    "patewayai",
    "pipellm",
    "qianfan_coding",
    "relaxycode",
    "rightcode",
    "runapi",
    "shengsuanyun",
    "siliconflow",
    "siliconflow_en",
    "sssaicode",
    "stepfun",
    "stepfun_en",
    "therouter",
    "xiaomi_mimo",
    "xiaomi_mimo_token_plan",
    "zhipu_glm",
    "zhipu_glm_en",
];

#[derive(Debug, Clone)]
pub struct RepairOptions {
    pub codex_dir: PathBuf,
    pub target_provider_id: Option<String>,
    pub dry_run: bool,
}

impl Default for RepairOptions {
    fn default() -> Self {
        Self {
            codex_dir: default_codex_dir(),
            target_provider_id: None,
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairOutcome {
    pub codex_dir: PathBuf,
    pub target_provider_id: String,
    pub target_provider_source: String,
    pub source_provider_ids: Vec<String>,
    pub scanned_jsonl_files: usize,
    pub migrated_jsonl_files: usize,
    pub migrated_jsonl_lines: usize,
    pub migrated_state_rows: usize,
    pub rebuilt_session_index_entries: usize,
    pub backup_root: Option<PathBuf>,
    pub dry_run: bool,
    pub skipped_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartOutcome {
    pub killed_processes: usize,
    pub launched: bool,
    pub launch_method: Option<String>,
    pub executable: Option<PathBuf>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CcSwitchDetection {
    pub cc_switch_dir: PathBuf,
    pub db_path: PathBuf,
    pub settings_path: PathBuf,
    pub db_exists: bool,
    pub settings_exists: bool,
    pub codex_override_dir: Option<PathBuf>,
    pub current_codex_provider_id: Option<String>,
    pub current_codex_provider_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderImportDraft {
    pub provider_id: String,
    pub provider_name: String,
    pub import_kind: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub oauth_account_id: Option<String>,
    pub api_format: String,
    pub model: String,
    pub settings_config: Value,
    pub meta: Value,
}

#[derive(Debug, Clone)]
pub struct ProviderImportOptions {
    pub cc_switch_dir: PathBuf,
    pub provider_id: Option<String>,
    pub provider_name: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub api_format: Option<String>,
    pub model: Option<String>,
    pub json_text: Option<String>,
}

impl Default for ProviderImportOptions {
    fn default() -> Self {
        Self {
            cc_switch_dir: default_cc_switch_dir(),
            provider_id: None,
            provider_name: None,
            base_url: None,
            api_key: None,
            api_format: None,
            model: None,
            json_text: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderImportOutcome {
    pub cc_switch_db_path: PathBuf,
    pub provider_id: String,
    pub provider_name: String,
    pub import_kind: String,
    pub base_url: Option<String>,
    pub oauth_account_id: Option<String>,
    pub api_format: String,
    pub created: bool,
    pub updated: bool,
    pub backup_path: Option<PathBuf>,
    pub oauth_store_path: Option<PathBuf>,
    pub message: String,
}

#[derive(Debug, Error)]
pub enum RepairError {
    #[error("could not find a home directory")]
    HomeDirUnavailable,
    #[error("Codex directory does not exist: {0}")]
    CodexDirMissing(PathBuf),
    #[error("cc-switch database does not exist: {0}")]
    CcSwitchDbMissing(PathBuf),
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("SQLite error at {path}: {source}")]
    Sqlite {
        path: PathBuf,
        source: rusqlite::Error,
    },
    #[error("JSON parse failed at {path}: {source}")]
    JsonParse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("JSON serialization failed: {0}")]
    JsonSerialize(serde_json::Error),
    #[error("TOML parse failed at {path}: {source}")]
    TomlParse {
        path: PathBuf,
        source: toml_edit::TomlError,
    },
    #[error("provider import failed: {0}")]
    ProviderImport(String),
    #[error("failed to restart Codex: {0}")]
    Restart(String),
}

impl RepairError {
    fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    fn sqlite(path: impl Into<PathBuf>, source: rusqlite::Error) -> Self {
        Self::Sqlite {
            path: path.into(),
            source,
        }
    }
}

pub fn default_codex_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".codex")
}

pub fn default_cc_switch_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".cc-switch")
}

pub fn resolve_default_codex_dir() -> PathBuf {
    detect_cc_switch()
        .codex_override_dir
        .unwrap_or_else(default_codex_dir)
}

pub fn detect_cc_switch() -> CcSwitchDetection {
    let cc_switch_dir = default_cc_switch_dir();
    let db_path = cc_switch_dir.join(CC_SWITCH_DB_FILENAME);
    let settings_path = cc_switch_dir.join("settings.json");
    let settings = read_json_value_optional(&settings_path).ok().flatten();
    let codex_override_dir = settings
        .as_ref()
        .and_then(|value| pick_string(value, &[&["codexConfigDir"], &["codex_config_dir"]]))
        .map(PathBuf::from);
    let current_codex_provider_id = settings
        .as_ref()
        .and_then(|value| {
            pick_string(
                value,
                &[&["currentProviderCodex"], &["current_provider_codex"]],
            )
        })
        .or_else(|| {
            if !db_path.exists() {
                return None;
            }
            let conn = Connection::open(&db_path).ok()?;
            current_codex_provider_id_from_db(&conn).ok().flatten()
        });
    let current_codex_provider_name = current_codex_provider_id.as_ref().and_then(|provider_id| {
        if !db_path.exists() {
            return None;
        }
        let conn = Connection::open(&db_path).ok()?;
        provider_name_from_db(&conn, provider_id).ok().flatten()
    });

    CcSwitchDetection {
        cc_switch_dir,
        db_path: db_path.clone(),
        settings_path: settings_path.clone(),
        db_exists: db_path.exists(),
        settings_exists: settings_path.exists(),
        codex_override_dir,
        current_codex_provider_id,
        current_codex_provider_name,
    }
}

pub fn repair_codex_history(options: RepairOptions) -> Result<RepairOutcome, RepairError> {
    if !options.codex_dir.exists() {
        return Err(RepairError::CodexDirMissing(options.codex_dir));
    }

    let (target_provider_id, target_provider_source) =
        resolve_target_provider_id(&options.codex_dir, options.target_provider_id.as_deref());
    let jsonl_files = collect_jsonl_files(&options.codex_dir);
    let mut source_provider_ids =
        collect_jsonl_model_provider_ids(&jsonl_files, &target_provider_id);
    source_provider_ids.extend(collect_state_model_provider_ids(
        &options.codex_dir,
        &target_provider_id,
    )?);
    source_provider_ids.retain(|id| is_sync_source_provider_id(id, &target_provider_id));

    let mut backup_root = None;
    let mut migrated_jsonl_files = 0;
    let mut migrated_jsonl_lines = 0;
    for file_path in &jsonl_files {
        let result = rewrite_jsonl_file(
            file_path,
            &options.codex_dir,
            &target_provider_id,
            &source_provider_ids,
            &mut backup_root,
            options.dry_run,
        )?;
        if result.changed {
            migrated_jsonl_files += 1;
            migrated_jsonl_lines += result.changed_lines;
        }
    }

    let migrated_state_rows = migrate_state_db(
        &options.codex_dir,
        &target_provider_id,
        &source_provider_ids,
        &mut backup_root,
        options.dry_run,
    )?;

    let rebuilt_session_index_entries =
        rebuild_session_index(&options.codex_dir, &mut backup_root, options.dry_run)?;

    let skipped_reason = if migrated_jsonl_files == 0
        && migrated_state_rows == 0
        && rebuilt_session_index_entries == 0
    {
        Some("no_changes_needed".to_string())
    } else {
        None
    };

    Ok(RepairOutcome {
        codex_dir: options.codex_dir,
        target_provider_id,
        target_provider_source,
        source_provider_ids: source_provider_ids.into_iter().collect(),
        scanned_jsonl_files: jsonl_files.len(),
        migrated_jsonl_files,
        migrated_jsonl_lines,
        migrated_state_rows,
        rebuilt_session_index_entries,
        backup_root,
        dry_run: options.dry_run,
        skipped_reason,
    })
}

fn resolve_target_provider_id(codex_dir: &Path, explicit: Option<&str>) -> (String, String) {
    if let Some(value) = explicit.and_then(normalize_non_empty) {
        return (value, "explicit".to_string());
    }
    if let Some(value) = active_codex_model_provider_id_from_cc_switch() {
        return (value, "cc_switch_current_provider".to_string());
    }
    if let Ok(Some(value)) = active_codex_model_provider_id(codex_dir) {
        return (value, "codex_config".to_string());
    }
    (
        CODEX_DEFAULT_MODEL_PROVIDER_ID.to_string(),
        "default".to_string(),
    )
}

fn active_codex_model_provider_id_from_cc_switch() -> Option<String> {
    let detected = detect_cc_switch();
    if !detected.db_path.exists() {
        return None;
    }
    let conn = Connection::open(detected.db_path).ok()?;
    let provider_id = detected
        .current_codex_provider_id
        .or_else(|| current_codex_provider_id_from_db(&conn).ok().flatten())?;
    let settings_config = provider_settings_config_from_db(&conn, &provider_id)
        .ok()
        .flatten()?;
    let config_text = settings_config.get("config").and_then(Value::as_str)?;
    active_model_provider_from_toml(config_text)
}

fn current_codex_provider_id_from_db(conn: &Connection) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT id FROM providers WHERE app_type = 'codex' AND is_current = 1 LIMIT 1",
        [],
        |row| row.get(0),
    )
    .optional()
}

fn provider_name_from_db(
    conn: &Connection,
    provider_id: &str,
) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT name FROM providers WHERE app_type = 'codex' AND id = ? LIMIT 1",
        [provider_id],
        |row| row.get(0),
    )
    .optional()
}

fn provider_settings_config_from_db(
    conn: &Connection,
    provider_id: &str,
) -> Result<Option<Value>, rusqlite::Error> {
    let text: Option<String> = conn
        .query_row(
            "SELECT settings_config FROM providers WHERE app_type = 'codex' AND id = ? LIMIT 1",
            [provider_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(text.and_then(|text| serde_json::from_str(&text).ok()))
}

fn active_codex_model_provider_id(codex_dir: &Path) -> Result<Option<String>, RepairError> {
    let config_path = codex_dir.join("config.toml");
    if !config_path.exists() {
        return Ok(None);
    }

    let config_text =
        fs::read_to_string(&config_path).map_err(|e| RepairError::io(&config_path, e))?;
    Ok(active_model_provider_from_toml(&config_text))
}

fn active_model_provider_from_toml(config_text: &str) -> Option<String> {
    config_text
        .parse::<DocumentMut>()
        .ok()?
        .get("model_provider")
        .and_then(|item| item.as_str())
        .and_then(normalize_non_empty)
}

fn collect_jsonl_files(codex_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in [
        codex_dir.join("sessions"),
        codex_dir.join("archived_sessions"),
    ] {
        if !root.exists() {
            continue;
        }

        for entry in WalkDir::new(root)
            .max_depth(10)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            let path = entry.into_path();
            if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn collect_jsonl_model_provider_ids(
    files: &[PathBuf],
    target_provider_id: &str,
) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for file_path in files {
        let Ok(file) = fs::File::open(file_path) else {
            continue;
        };
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let Some(provider_id) = session_meta_model_provider(&line) else {
                continue;
            };
            if provider_id != target_provider_id {
                ids.insert(provider_id);
            }
        }
    }
    ids
}

fn collect_state_model_provider_ids(
    codex_dir: &Path,
    target_provider_id: &str,
) -> Result<BTreeSet<String>, RepairError> {
    let db_path = codex_dir.join(CODEX_STATE_DB_FILENAME);
    let mut ids = BTreeSet::new();
    if !db_path.exists() {
        return Ok(ids);
    }

    let conn = open_sqlite_with_timeout(&db_path)?;
    if !threads_table_exists(&conn, &db_path)? {
        return Ok(ids);
    }

    let mut stmt = conn
        .prepare("SELECT DISTINCT model_provider FROM threads WHERE model_provider IS NOT NULL")
        .map_err(|e| RepairError::sqlite(&db_path, e))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| RepairError::sqlite(&db_path, e))?;
    for row in rows {
        let id = row.map_err(|e| RepairError::sqlite(&db_path, e))?;
        let id = id.trim();
        if !id.is_empty() && id != target_provider_id {
            ids.insert(id.to_string());
        }
    }
    Ok(ids)
}

fn is_sync_source_provider_id(id: &str, target_provider_id: &str) -> bool {
    if id == target_provider_id {
        return false;
    }
    if id == CODEX_LOCAL_ACCESS_PROVIDER_ID {
        return true;
    }
    if is_openai_custom_pair(id, target_provider_id) {
        return true;
    }
    is_legacy_provider_id(id)
}

fn is_openai_custom_pair(source_provider_id: &str, target_provider_id: &str) -> bool {
    matches!(
        (source_provider_id, target_provider_id),
        ("openai", "custom") | ("custom", "openai")
    )
}

fn is_legacy_provider_id(id: &str) -> bool {
    LEGACY_PROVIDER_IDS.iter().any(|legacy| *legacy == id)
}

#[derive(Debug, Clone, Copy)]
struct RewriteJsonlResult {
    changed: bool,
    changed_lines: usize,
}

fn rewrite_jsonl_file(
    path: &Path,
    codex_dir: &Path,
    target_provider_id: &str,
    source_provider_ids: &BTreeSet<String>,
    backup_root: &mut Option<PathBuf>,
    dry_run: bool,
) -> Result<RewriteJsonlResult, RepairError> {
    if source_provider_ids.is_empty() {
        return Ok(RewriteJsonlResult {
            changed: false,
            changed_lines: 0,
        });
    }

    let text = fs::read_to_string(path).map_err(|e| RepairError::io(path, e))?;
    let mut changed_lines = 0;
    let mut output = String::with_capacity(text.len());

    for segment in text.split_inclusive('\n') {
        let (line, ending) = segment
            .strip_suffix('\n')
            .map(|line| (line, "\n"))
            .unwrap_or((segment, ""));
        if let Some(rewritten) =
            rewrite_session_meta_line(line, target_provider_id, source_provider_ids)
        {
            output.push_str(&rewritten);
            output.push_str(ending);
            changed_lines += 1;
        } else {
            output.push_str(line);
            output.push_str(ending);
        }
    }

    if changed_lines == 0 {
        return Ok(RewriteJsonlResult {
            changed: false,
            changed_lines: 0,
        });
    }

    if !dry_run {
        let root = ensure_backup_root(codex_dir, backup_root)?;
        backup_file(path, codex_dir, &root.join("jsonl"))?;
        atomic_write(path, output.as_bytes())?;
    }

    Ok(RewriteJsonlResult {
        changed: true,
        changed_lines,
    })
}

fn session_meta_model_provider(line: &str) -> Option<String> {
    if !line.contains("\"session_meta\"") || !line.contains("\"model_provider\"") {
        return None;
    }

    let value: Value = serde_json::from_str(line).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }

    value
        .get("payload")?
        .get("model_provider")?
        .as_str()
        .and_then(normalize_non_empty)
}

fn rewrite_session_meta_line(
    line: &str,
    target_provider_id: &str,
    source_provider_ids: &BTreeSet<String>,
) -> Option<String> {
    let current = session_meta_model_provider(line)?;
    if current == target_provider_id || !source_provider_ids.contains(&current) {
        return None;
    }

    let mut value: Value = serde_json::from_str(line).ok()?;
    let payload = value.get_mut("payload")?.as_object_mut()?;
    payload.insert(
        "model_provider".to_string(),
        Value::String(target_provider_id.to_string()),
    );
    serde_json::to_string(&value).ok()
}

fn migrate_state_db(
    codex_dir: &Path,
    target_provider_id: &str,
    source_provider_ids: &BTreeSet<String>,
    backup_root: &mut Option<PathBuf>,
    dry_run: bool,
) -> Result<usize, RepairError> {
    if source_provider_ids.is_empty() {
        return Ok(0);
    }

    let db_path = codex_dir.join(CODEX_STATE_DB_FILENAME);
    if !db_path.exists() {
        return Ok(0);
    }

    let mut conn = open_sqlite_with_timeout(&db_path)?;
    if !threads_table_exists(&conn, &db_path)? {
        return Ok(0);
    }

    let mut rows = 0usize;
    for source_id in source_provider_ids {
        rows += conn
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE model_provider = ?",
                [source_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| RepairError::sqlite(&db_path, e))? as usize;
    }

    if rows == 0 || dry_run {
        return Ok(rows);
    }

    let root = ensure_backup_root(codex_dir, backup_root)?;
    backup_sqlite_database(&db_path, &root.join("state").join(CODEX_STATE_DB_FILENAME))?;

    let tx = conn
        .transaction()
        .map_err(|e| RepairError::sqlite(&db_path, e))?;
    for source_id in source_provider_ids {
        tx.execute(
            "UPDATE threads SET model_provider = ? WHERE model_provider = ?",
            [target_provider_id, source_id],
        )
        .map_err(|e| RepairError::sqlite(&db_path, e))?;
    }
    tx.commit().map_err(|e| RepairError::sqlite(&db_path, e))?;
    Ok(rows)
}

fn threads_table_exists(conn: &Connection, db_path: &Path) -> Result<bool, RepairError> {
    let exists = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'threads'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|e| RepairError::sqlite(db_path, e))?;
    Ok(exists > 0)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct SessionIndexEntry {
    id: String,
    thread_name: String,
    updated_at: String,
}

fn rebuild_session_index(
    codex_dir: &Path,
    backup_root: &mut Option<PathBuf>,
    dry_run: bool,
) -> Result<usize, RepairError> {
    let index_path = codex_dir.join("session_index.jsonl");
    let mut entries_by_id = read_existing_session_index_entries(&index_path)?;
    let mut changed_entries = 0;

    for file_path in collect_jsonl_files(codex_dir) {
        let Some(entry) = session_index_entry_from_jsonl(&file_path)? else {
            continue;
        };

        if let Some(existing) = entries_by_id.get_mut(&entry.id) {
            if entry.updated_at > existing.updated_at {
                existing.updated_at = entry.updated_at;
                changed_entries += 1;
            }
        } else {
            entries_by_id.insert(entry.id.clone(), entry);
            changed_entries += 1;
        }
    }

    if changed_entries == 0 {
        return Ok(0);
    }

    if dry_run {
        return Ok(changed_entries);
    }

    if index_path.exists() {
        let root = ensure_backup_root(codex_dir, backup_root)?;
        backup_file(&index_path, codex_dir, &root.join("session_index"))?;
    } else if let Some(parent) = index_path.parent() {
        fs::create_dir_all(parent).map_err(|e| RepairError::io(parent, e))?;
    }

    let mut entries: Vec<_> = entries_by_id.into_values().collect();
    entries.sort_by(|a, b| {
        a.updated_at
            .cmp(&b.updated_at)
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut output = String::new();
    for entry in entries {
        output.push_str(&serde_json::to_string(&entry).map_err(RepairError::JsonSerialize)?);
        output.push('\n');
    }
    atomic_write(&index_path, output.as_bytes())?;
    Ok(changed_entries)
}

fn read_existing_session_index_entries(
    index_path: &Path,
) -> Result<BTreeMap<String, SessionIndexEntry>, RepairError> {
    let mut entries = BTreeMap::new();
    if !index_path.exists() {
        return Ok(entries);
    }

    let file = fs::File::open(index_path).map_err(|e| RepairError::io(index_path, e))?;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(entry) = serde_json::from_str::<SessionIndexEntry>(&line) else {
            continue;
        };
        if !entry.id.trim().is_empty() {
            entries.insert(entry.id.clone(), entry);
        }
    }
    Ok(entries)
}

fn session_index_entry_from_jsonl(path: &Path) -> Result<Option<SessionIndexEntry>, RepairError> {
    let file = fs::File::open(path).map_err(|e| RepairError::io(path, e))?;

    let mut id = None;
    let mut cwd = None;
    let mut thread_name = None;
    let mut first_timestamp = None;
    let mut updated_at = None;

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        if let Some(timestamp) = value.get("timestamp").and_then(Value::as_str) {
            if first_timestamp.is_none() {
                first_timestamp = Some(timestamp.to_string());
            }
            updated_at = Some(timestamp.to_string());
        }

        if value.get("type").and_then(Value::as_str) == Some("session_meta") {
            if let Some(payload) = value.get("payload") {
                if id.is_none() {
                    id = payload
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
                if cwd.is_none() {
                    cwd = payload
                        .get("cwd")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
                if let Some(timestamp) = payload.get("timestamp").and_then(Value::as_str) {
                    first_timestamp.get_or_insert_with(|| timestamp.to_string());
                    updated_at.get_or_insert_with(|| timestamp.to_string());
                }
            }
            continue;
        }

        if thread_name.is_none()
            && value.get("type").and_then(Value::as_str) == Some("response_item")
        {
            let Some(payload) = value.get("payload") else {
                continue;
            };
            if payload.get("type").and_then(Value::as_str) == Some("message")
                && payload.get("role").and_then(Value::as_str) == Some("user")
            {
                let text = extract_index_text(payload.get("content"))
                    .trim()
                    .to_string();
                if !text.is_empty()
                    && !text.starts_with("# AGENTS.md")
                    && !text.starts_with("<environment_context>")
                {
                    thread_name = Some(text);
                }
            }
        }
    }

    let Some(id) = id else {
        return Ok(None);
    };
    let updated_at = updated_at
        .or(first_timestamp)
        .unwrap_or_else(|| Utc::now().to_rfc3339());
    let thread_name = thread_name
        .or_else(|| {
            cwd.as_deref()
                .and_then(|value| Path::new(value).file_name())
                .map(|value| value.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| id.clone());

    Ok(Some(SessionIndexEntry {
        id,
        thread_name,
        updated_at,
    }))
}

fn extract_index_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .or_else(|| item.get("content"))
                    .or_else(|| item.get("input_text"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Some(Value::Object(map)) => map
            .get("text")
            .or_else(|| map.get("content"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

pub fn parse_provider_import_draft(
    options: &ProviderImportOptions,
) -> Result<ProviderImportDraft, RepairError> {
    let imported_json =
        match options.json_text.as_deref().and_then(normalize_non_empty) {
            Some(text) => Some(serde_json::from_str::<Value>(&text).map_err(|source| {
                RepairError::JsonParse {
                    path: PathBuf::from("<import-json>"),
                    source,
                }
            })?),
            None => None,
        };
    let json_ref = imported_json.as_ref();

    if let Some(oauth_auth) = json_ref.and_then(extract_codex_oauth_auth) {
        let account_id =
            extract_oauth_account_id(json_ref.unwrap(), &oauth_auth).unwrap_or_else(|| {
                format!(
                    "openai_account_{}",
                    &secret_hash(oauth_auth.to_string())[..8]
                )
            });
        let provider_name = options
            .provider_name
            .as_deref()
            .and_then(normalize_non_empty)
            .or_else(|| extract_oauth_account_name(&oauth_auth))
            .or_else(|| json_ref.and_then(extract_provider_name))
            .unwrap_or_else(|| "OpenAI Account".to_string());
        let provider_id = options
            .provider_id
            .as_deref()
            .and_then(sanitize_provider_id)
            .unwrap_or_else(|| derive_oauth_provider_id(&provider_name, &account_id));
        let model = options
            .model
            .as_deref()
            .and_then(normalize_non_empty)
            .or_else(|| json_ref.and_then(extract_model))
            .unwrap_or_else(|| DEFAULT_CODEX_MODEL.to_string());
        let api_format = "openai_responses".to_string();
        let settings_config = build_cc_switch_openai_account_settings_config(oauth_auth.clone());
        let meta = json!({
            "commonConfigEnabled": true,
            "endpointAutoSelect": true,
            "apiFormat": api_format,
            "importedBy": "codex-history-repair",
        });

        return Ok(ProviderImportDraft {
            provider_id,
            provider_name,
            import_kind: "openai_account".to_string(),
            base_url: None,
            api_key: None,
            oauth_account_id: Some(account_id),
            api_format,
            model,
            settings_config,
            meta,
        });
    }

    Err(RepairError::ProviderImport(
        "only CPA/cockpit/sub2api OpenAI account JSON imports are supported; base_url + API key import is intentionally disabled".to_string(),
    ))
}

pub fn import_cc_switch_provider(
    options: ProviderImportOptions,
) -> Result<ProviderImportOutcome, RepairError> {
    let draft = parse_provider_import_draft(&options)?;
    let db_path = options.cc_switch_dir.join(CC_SWITCH_DB_FILENAME);
    if !db_path.exists() {
        return Err(RepairError::CcSwitchDbMissing(db_path));
    }

    let backup_path = Some(backup_cc_switch_db(&db_path)?);
    let conn = open_sqlite_with_timeout(&db_path)?;
    ensure_cc_switch_provider_table(&conn, &db_path)?;
    let existing = find_existing_provider_id(&conn, &draft)?;
    let now = Utc::now().timestamp_millis();
    let provider_id = existing.unwrap_or_else(|| draft.provider_id.clone());
    let created = provider_id == draft.provider_id
        && provider_row_exists(&conn, &provider_id)
            .map(|exists| !exists)
            .unwrap_or(false);
    let sort_index = next_sort_index(&conn).unwrap_or(0);
    let is_openai_account = draft.import_kind == "openai_account";
    let category = if is_openai_account {
        "official"
    } else {
        "custom"
    };
    let website_url = if is_openai_account {
        Some("https://chatgpt.com/codex")
    } else {
        None
    };

    conn.execute(
        "INSERT INTO providers (
            id, app_type, name, settings_config, website_url, category,
            created_at, sort_index, notes, icon, icon_color, meta, is_current, in_failover_queue
        ) VALUES (?1, 'codex', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, 0)
        ON CONFLICT(id, app_type) DO UPDATE SET
            name = excluded.name,
            settings_config = excluded.settings_config,
            website_url = excluded.website_url,
            category = excluded.category,
            notes = excluded.notes,
            icon = excluded.icon,
            icon_color = excluded.icon_color,
            meta = excluded.meta",
        params![
            provider_id,
            draft.provider_name,
            serde_json::to_string(&draft.settings_config).map_err(RepairError::JsonSerialize)?,
            website_url,
            category,
            now,
            sort_index,
            "Imported by codex-history-repair",
            "openai",
            "#00A67E",
            serde_json::to_string(&draft.meta).map_err(RepairError::JsonSerialize)?,
        ],
    )
    .map_err(|e| RepairError::sqlite(&db_path, e))?;

    let oauth_store_path = None;

    let updated = !created;
    Ok(ProviderImportOutcome {
        cc_switch_db_path: db_path,
        provider_id,
        provider_name: draft.provider_name,
        import_kind: draft.import_kind,
        base_url: draft.base_url,
        oauth_account_id: draft.oauth_account_id,
        api_format: draft.api_format,
        created,
        updated,
        backup_path,
        oauth_store_path,
        message: if created {
            "OpenAI account imported into cc-switch. It was not switched on automatically."
                .to_string()
        } else {
            "OpenAI account updated in cc-switch. Current provider was not changed.".to_string()
        },
    })
}

fn ensure_cc_switch_provider_table(conn: &Connection, db_path: &Path) -> Result<(), RepairError> {
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'providers'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| RepairError::sqlite(db_path, e))?;
    if exists == 0 {
        return Err(RepairError::ProviderImport(
            "cc-switch providers table not found".to_string(),
        ));
    }
    Ok(())
}

fn find_existing_provider_id(
    conn: &Connection,
    draft: &ProviderImportDraft,
) -> Result<Option<String>, RepairError> {
    if provider_row_exists(conn, &draft.provider_id)? {
        return Ok(Some(draft.provider_id.clone()));
    }
    if draft.import_kind == "openai_account" {
        return find_existing_oauth_provider_id(conn, draft);
    }
    Ok(None)
}

fn find_existing_oauth_provider_id(
    conn: &Connection,
    draft: &ProviderImportDraft,
) -> Result<Option<String>, RepairError> {
    let Some(target_account_id) = draft.oauth_account_id.as_deref() else {
        return Ok(None);
    };
    let mut stmt = conn
        .prepare("SELECT id, meta, settings_config FROM providers WHERE app_type = 'codex'")
        .map_err(|e| RepairError::sqlite(default_cc_switch_dir().join(CC_SWITCH_DB_FILENAME), e))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1).unwrap_or_default(),
                row.get::<_, String>(2).unwrap_or_default(),
            ))
        })
        .map_err(|e| RepairError::sqlite(default_cc_switch_dir().join(CC_SWITCH_DB_FILENAME), e))?;

    for row in rows {
        let (id, meta_text, settings_text) = row.map_err(|e| {
            RepairError::sqlite(default_cc_switch_dir().join(CC_SWITCH_DB_FILENAME), e)
        })?;
        let meta = serde_json::from_str::<Value>(&meta_text).unwrap_or(Value::Null);
        let bound_account_id = meta
            .pointer("/authBinding/accountId")
            .and_then(Value::as_str)
            .or_else(|| meta.get("githubAccountId").and_then(Value::as_str));
        if bound_account_id == Some(target_account_id) {
            return Ok(Some(id));
        }

        let settings = serde_json::from_str::<Value>(&settings_text).unwrap_or(Value::Null);
        let auth = settings.get("auth").unwrap_or(&Value::Null);
        if extract_oauth_account_id(auth, auth).as_deref() == Some(target_account_id) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

fn provider_row_exists(conn: &Connection, provider_id: &str) -> Result<bool, RepairError> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM providers WHERE app_type = 'codex' AND id = ?",
            [provider_id],
            |row| row.get(0),
        )
        .map_err(|e| RepairError::sqlite(default_cc_switch_dir().join(CC_SWITCH_DB_FILENAME), e))?;
    Ok(count > 0)
}

fn next_sort_index(conn: &Connection) -> Result<i64, rusqlite::Error> {
    conn.query_row(
        "SELECT COALESCE(MAX(sort_index), -1) + 1 FROM providers WHERE app_type = 'codex'",
        [],
        |row| row.get(0),
    )
}

fn build_cc_switch_openai_account_settings_config(auth: Value) -> Value {
    json!({
        "auth": auth,
        "config": "",
    })
}

fn extract_codex_oauth_auth(value: &Value) -> Option<Value> {
    let candidate = if let Some(accounts) = value.get("accounts").and_then(Value::as_array) {
        accounts
            .iter()
            .find(|account| {
                account
                    .get("platform")
                    .and_then(Value::as_str)
                    .is_none_or(|platform| platform.eq_ignore_ascii_case("openai"))
                    && account
                        .get("type")
                        .and_then(Value::as_str)
                        .is_none_or(|kind| kind.eq_ignore_ascii_case("oauth"))
            })
            .unwrap_or_else(|| accounts.first().unwrap_or(value))
    } else {
        value
    };

    let credentials = candidate.get("credentials").unwrap_or(candidate);
    let extra = candidate.get("extra").unwrap_or(value);
    let access_token = pick_first_string(
        &[credentials, candidate, value],
        &[&["access_token"], &["token"]],
    );
    let id_token = pick_first_string(
        &[credentials, candidate, value],
        &[&["id_token"], &["idToken"]],
    );
    let session_token = pick_first_string(
        &[credentials, candidate, value],
        &[&["session_token"], &["sessionToken"]],
    );
    let refresh_token = pick_first_string(
        &[credentials, candidate, value],
        &[&["refresh_token"], &["refreshToken"]],
    );

    if access_token.is_none()
        && id_token.is_none()
        && session_token.is_none()
        && refresh_token.is_none()
    {
        return None;
    }

    let account_id = pick_first_string(
        &[credentials, extra, candidate, value],
        &[
            &["chatgpt_account_id"],
            &["account_id"],
            &["workspace_id"],
            &["chatgpt_user_id"],
        ],
    );
    let email = pick_first_string(
        &[credentials, extra, candidate, value],
        &[&["email"], &["name"]],
    );
    let name = pick_first_string(
        &[credentials, extra, candidate, value],
        &[&["name"], &["email"]],
    );
    let plan_type = pick_first_string(
        &[credentials, extra, candidate, value],
        &[&["chatgpt_plan_type"], &["plan_type"]],
    );
    let expired = pick_first_string(
        &[credentials, extra, candidate, value],
        &[&["expired"], &["expires_at"], &["expiresAt"]],
    );
    let last_refresh = pick_first_string(
        &[credentials, extra, candidate, value],
        &[&["last_refresh"], &["lastRefresh"]],
    );

    let mut tokens = serde_json::Map::new();
    insert_optional_string(&mut tokens, "access_token", access_token.clone());
    insert_optional_string(&mut tokens, "id_token", id_token.clone());
    insert_optional_string(&mut tokens, "refresh_token", refresh_token.clone());
    insert_optional_string(&mut tokens, "session_token", session_token.clone());
    insert_optional_string(&mut tokens, "account_id", account_id.clone());
    insert_optional_string(&mut tokens, "chatgpt_account_id", account_id.clone());
    insert_optional_string(&mut tokens, "email", email.clone());
    insert_optional_string(&mut tokens, "name", name);
    insert_optional_string(&mut tokens, "plan_type", plan_type.clone());
    insert_optional_string(&mut tokens, "expired", expired.clone());
    insert_optional_string(&mut tokens, "last_refresh", last_refresh.clone());

    let mut auth = serde_json::Map::new();
    auth.insert("OPENAI_API_KEY".to_string(), Value::Null);
    auth.insert("tokens".to_string(), Value::Object(tokens));
    insert_optional_string(&mut auth, "expired", expired);
    insert_optional_string(&mut auth, "last_refresh", last_refresh);
    Some(Value::Object(auth))
}

fn extract_oauth_account_id(source: &Value, auth: &Value) -> Option<String> {
    pick_first_string(
        &[auth, auth.get("tokens").unwrap_or(&Value::Null), source],
        &[
            &["chatgpt_account_id"],
            &["account_id"],
            &["tokens", "chatgpt_account_id"],
            &["tokens", "account_id"],
            &["credentials", "chatgpt_account_id"],
            &["credentials", "account_id"],
        ],
    )
}

fn extract_oauth_account_name(auth: &Value) -> Option<String> {
    pick_first_string(
        &[auth, auth.get("tokens").unwrap_or(&Value::Null)],
        &[
            &["name"],
            &["email"],
            &["tokens", "name"],
            &["tokens", "email"],
        ],
    )
}

fn pick_first_string(values: &[&Value], paths: &[&[&str]]) -> Option<String> {
    values.iter().find_map(|value| pick_string(value, paths))
}

fn insert_optional_string(
    map: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<String>,
) {
    if let Some(value) = value {
        map.insert(key.to_string(), Value::String(value));
    }
}

fn extract_provider_name(value: &Value) -> Option<String> {
    pick_string(
        value,
        &[
            &["providerName"],
            &["provider_name"],
            &["name"],
            &["label"],
            &["provider", "name"],
        ],
    )
}

fn extract_model(value: &Value) -> Option<String> {
    pick_string(value, &[&["model"], &["defaultModel"], &["default_model"]])
}

fn derive_oauth_provider_id(provider_name: &str, account_id: &str) -> String {
    let name = sanitize_provider_id(provider_name).unwrap_or_else(|| "chatgpt".to_string());
    format!("chr_openai_{}_{}", name, &secret_hash(account_id)[..8])
}

fn sanitize_provider_id(raw: &str) -> Option<String> {
    let mut output = String::new();
    let mut previous_separator = false;
    for ch in raw.trim().chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            previous_separator = false;
            ch.to_ascii_lowercase()
        } else if matches!(ch, '-' | '_') {
            if previous_separator {
                continue;
            }
            previous_separator = true;
            '_'
        } else {
            if previous_separator {
                continue;
            }
            previous_separator = true;
            '_'
        };
        output.push(mapped);
    }
    let output = output.trim_matches('_').to_string();
    if output.is_empty() {
        return None;
    }
    if output
        .chars()
        .next()
        .map(|ch| ch.is_ascii_alphabetic())
        .unwrap_or(false)
    {
        Some(output)
    } else {
        Some(format!("provider_{output}"))
    }
}

fn pick_string(value: &Value, paths: &[&[&str]]) -> Option<String> {
    for path in paths {
        let mut cursor = value;
        let mut found = true;
        for key in *path {
            match cursor.get(*key) {
                Some(next) => cursor = next,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if found {
            if let Some(text) = cursor.as_str().and_then(normalize_non_empty) {
                return Some(text);
            }
        }
    }
    match value {
        Value::Object(map) => {
            for child in map.values() {
                if let Some(text) = pick_string(child, paths) {
                    return Some(text);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(|item| pick_string(item, paths)),
        _ => None,
    }
}

fn normalize_non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn secret_hash(value: impl AsRef<str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_ref().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn read_json_value_optional(path: &Path) -> Result<Option<Value>, RepairError> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).map_err(|e| RepairError::io(path, e))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|source| RepairError::JsonParse {
            path: path.to_path_buf(),
            source,
        })
}

fn backup_cc_switch_db(db_path: &Path) -> Result<PathBuf, RepairError> {
    let parent = db_path.parent().unwrap_or_else(|| Path::new("."));
    let backup_dir = parent.join("codex-history-repair-backups");
    fs::create_dir_all(&backup_dir).map_err(|e| RepairError::io(&backup_dir, e))?;
    let backup_path = backup_dir.join(format!(
        "cc-switch-{}.db",
        Local::now().format("%Y%m%d-%H%M%S")
    ));
    backup_sqlite_database(db_path, &backup_path)?;
    Ok(backup_path)
}

pub fn restart_codex() -> Result<RestartOutcome, RepairError> {
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);

    let mut executable = None;
    let mut killed_processes = 0;
    for process in system.processes().values() {
        if !is_codex_process_name(&process.name().to_string_lossy()) {
            continue;
        }

        if executable.is_none() {
            executable = process.exe().map(Path::to_path_buf);
        }

        if process.kill() {
            killed_processes += 1;
        }
    }

    if killed_processes > 0 {
        thread::sleep(Duration::from_millis(1200));
    }

    if let Some(executable) = executable.as_deref() {
        if launch_executable(executable).is_ok() {
            return Ok(RestartOutcome {
                killed_processes,
                launched: true,
                launch_method: Some("running_process_executable".to_string()),
                executable: Some(executable.to_path_buf()),
                message: "Restarted Codex from the previously running executable.".to_string(),
            });
        }
    }

    if let Some(method) = launch_platform_default() {
        return Ok(RestartOutcome {
            killed_processes,
            launched: true,
            launch_method: Some(method),
            executable,
            message: "Started Codex with the platform default launcher.".to_string(),
        });
    }

    Ok(RestartOutcome {
        killed_processes,
        launched: false,
        launch_method: None,
        executable,
        message:
            "Could not find a Codex launcher. The repair is complete; please reopen Codex manually."
                .to_string(),
    })
}

fn is_codex_process_name(name: &str) -> bool {
    matches!(name, "Codex" | "Codex.exe" | "codex" | "codex.exe")
}

fn launch_executable(executable: &Path) -> Result<(), RepairError> {
    #[cfg(target_os = "macos")]
    {
        if let Some(app_path) = macos_app_bundle_path(executable) {
            Command::new("open")
                .arg(app_path)
                .spawn()
                .map_err(|e| RepairError::Restart(e.to_string()))?;
            return Ok(());
        }
    }

    Command::new(executable)
        .spawn()
        .map_err(|e| RepairError::Restart(e.to_string()))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_app_bundle_path(executable: &Path) -> Option<PathBuf> {
    executable
        .ancestors()
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("app"))
        .map(Path::to_path_buf)
}

fn launch_platform_default() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        if command_ok(Command::new("open").args(["-a", "Codex"])) {
            return Some("open -a Codex".to_string());
        }
    }

    #[cfg(target_os = "windows")]
    {
        if command_ok(Command::new("powershell").args([
            "-NoProfile",
            "-Command",
            "Start-Process Codex",
        ])) {
            return Some("powershell Start-Process Codex".to_string());
        }
        if command_ok(Command::new("cmd").args(["/C", "start", "", "Codex"])) {
            return Some("cmd start Codex".to_string());
        }
    }

    #[cfg(target_os = "linux")]
    {
        for desktop_id in ["codex.desktop", "Codex.desktop"] {
            if command_ok(Command::new("gtk-launch").arg(desktop_id)) {
                return Some(format!("gtk-launch {desktop_id}"));
            }
        }
        for command in ["codex", "Codex"] {
            if Command::new(command).spawn().is_ok() {
                return Some(command.to_string());
            }
        }
    }

    None
}

fn command_ok(command: &mut Command) -> bool {
    command
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn create_backup_root(codex_dir: &Path) -> Result<PathBuf, RepairError> {
    let timestamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
    let backup_root = codex_dir
        .join("history_repair_backups")
        .join(format!("codex-history-repair-{timestamp}"));
    fs::create_dir_all(&backup_root).map_err(|e| RepairError::io(&backup_root, e))?;
    Ok(backup_root)
}

fn ensure_backup_root(
    codex_dir: &Path,
    backup_root: &mut Option<PathBuf>,
) -> Result<PathBuf, RepairError> {
    if let Some(root) = backup_root {
        return Ok(root.clone());
    }
    let root = create_backup_root(codex_dir)?;
    *backup_root = Some(root.clone());
    Ok(root)
}

fn backup_file(path: &Path, codex_dir: &Path, backup_root: &Path) -> Result<(), RepairError> {
    let relative_path = path.strip_prefix(codex_dir).unwrap_or(path);
    let backup_path = backup_root.join(relative_path);
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent).map_err(|e| RepairError::io(parent, e))?;
    }
    fs::copy(path, &backup_path).map_err(|e| RepairError::io(&backup_path, e))?;
    Ok(())
}

fn backup_sqlite_database(source_path: &Path, backup_path: &Path) -> Result<(), RepairError> {
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent).map_err(|e| RepairError::io(parent, e))?;
    }
    if backup_path.exists() {
        fs::remove_file(backup_path).map_err(|e| RepairError::io(backup_path, e))?;
    }
    let source = open_sqlite_with_timeout(source_path)?;
    let mut dest =
        Connection::open(backup_path).map_err(|e| RepairError::sqlite(backup_path, e))?;
    let backup =
        Backup::new(&source, &mut dest).map_err(|e| RepairError::sqlite(source_path, e))?;
    backup
        .run_to_completion(32, Duration::from_millis(30), None)
        .map_err(|e| RepairError::sqlite(source_path, e))?;
    Ok(())
}

fn open_sqlite_with_timeout(path: &Path) -> Result<Connection, RepairError> {
    let conn = Connection::open(path).map_err(|e| RepairError::sqlite(path, e))?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|e| RepairError::sqlite(path, e))?;
    Ok(conn)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), RepairError> {
    let tmp_path = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("tmp")
    ));
    fs::write(&tmp_path, bytes).map_err(|e| RepairError::io(&tmp_path, e))?;
    fs::rename(&tmp_path, path).map_err(|e| RepairError::io(path, e))?;
    Ok(())
}

trait OptionalRow<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalRow<T> for Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn repairs_jsonl_state_and_index() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let session_dir = codex_dir.join("sessions/2026/06/06");
        fs::create_dir_all(&session_dir).expect("create sessions");
        fs::write(
            codex_dir.join("config.toml"),
            "model_provider = \"custom\"\n",
        )
        .expect("write config");
        fs::write(
            session_dir.join("rollout.jsonl"),
            concat!(
                "{\"timestamp\":\"2026-06-06T10:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"cwd\":\"/tmp/project\",\"model_provider\":\"ccswitch\"}}\n",
                "{\"timestamp\":\"2026-06-06T10:01:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"Hello Codex\"}}\n"
            ),
        )
        .expect("write jsonl");
        fs::write(
            session_dir.join("openai.jsonl"),
            "{\"timestamp\":\"2026-06-06T11:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"s-openai\",\"cwd\":\"/tmp/project\",\"model_provider\":\"openai\"}}\n",
        )
        .expect("write openai jsonl");

        let conn = Connection::open(codex_dir.join(CODEX_STATE_DB_FILENAME)).expect("open db");
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL);
             INSERT INTO threads (id, model_provider) VALUES ('s1', 'ccswitch'), ('s2', 'custom'), ('s-openai', 'openai');",
        )
        .expect("seed db");
        drop(conn);

        let outcome = repair_codex_history(RepairOptions {
            codex_dir: codex_dir.clone(),
            target_provider_id: Some("custom".to_string()),
            dry_run: false,
        })
        .expect("repair");

        assert_eq!(outcome.target_provider_id, "custom");
        assert_eq!(outcome.migrated_jsonl_files, 2);
        assert_eq!(outcome.migrated_jsonl_lines, 2);
        assert_eq!(outcome.migrated_state_rows, 2);
        assert_eq!(outcome.rebuilt_session_index_entries, 2);
        assert_eq!(
            outcome.source_provider_ids,
            vec!["ccswitch".to_string(), "openai".to_string()]
        );
        assert!(outcome.backup_root.as_ref().expect("backup root").exists());

        let session_text =
            fs::read_to_string(session_dir.join("rollout.jsonl")).expect("read jsonl");
        assert!(session_text.contains("\"model_provider\":\"custom\""));
        let openai_text =
            fs::read_to_string(session_dir.join("openai.jsonl")).expect("read openai jsonl");
        assert!(openai_text.contains("\"model_provider\":\"custom\""));

        let conn = Connection::open(codex_dir.join(CODEX_STATE_DB_FILENAME)).expect("open db");
        let custom_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE model_provider = 'custom'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(custom_rows, 3);
        let openai_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE model_provider = 'openai'",
                [],
                |row| row.get(0),
            )
            .expect("openai count");
        assert_eq!(openai_rows, 0);

        let index_text =
            fs::read_to_string(codex_dir.join("session_index.jsonl")).expect("read index");
        assert!(index_text.contains("\"thread_name\":\"Hello Codex\""));
    }

    #[test]
    fn dry_run_reports_without_writing() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let session_dir = codex_dir.join("sessions");
        fs::create_dir_all(&session_dir).expect("create sessions");
        let session_path = session_dir.join("rollout.jsonl");
        fs::write(
            &session_path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"ccswitch\"}}\n",
        )
        .expect("write jsonl");

        let outcome = repair_codex_history(RepairOptions {
            codex_dir,
            target_provider_id: Some("custom".to_string()),
            dry_run: true,
        })
        .expect("dry run");

        assert_eq!(outcome.migrated_jsonl_files, 1);
        assert_eq!(outcome.rebuilt_session_index_entries, 1);
        assert!(outcome.backup_root.is_none());
        let session_text = fs::read_to_string(session_path).expect("read jsonl");
        assert!(session_text.contains("\"model_provider\":\"ccswitch\""));
    }

    #[test]
    fn syncs_openai_bucket_when_target_is_custom() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let session_dir = codex_dir.join("sessions");
        fs::create_dir_all(&session_dir).expect("create sessions");
        let session_path = session_dir.join("rollout.jsonl");
        fs::write(
            &session_path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"openai\"}}\n",
        )
        .expect("write jsonl");

        let conn = Connection::open(codex_dir.join(CODEX_STATE_DB_FILENAME)).expect("open db");
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL);
             INSERT INTO threads (id, model_provider) VALUES ('s1', 'openai');",
        )
        .expect("seed db");
        drop(conn);

        let outcome = repair_codex_history(RepairOptions {
            codex_dir: codex_dir.clone(),
            target_provider_id: Some("custom".to_string()),
            dry_run: false,
        })
        .expect("repair");

        assert_eq!(outcome.migrated_jsonl_files, 1);
        assert_eq!(outcome.migrated_state_rows, 1);
        assert_eq!(outcome.source_provider_ids, vec!["openai".to_string()]);
        let session_text = fs::read_to_string(session_path).expect("read jsonl");
        assert!(session_text.contains("\"model_provider\":\"custom\""));

        let conn = Connection::open(codex_dir.join(CODEX_STATE_DB_FILENAME)).expect("open db");
        let provider: String = conn
            .query_row(
                "SELECT model_provider FROM threads WHERE id = 's1'",
                [],
                |row| row.get(0),
            )
            .expect("provider");
        assert_eq!(provider, "custom");
    }

    #[test]
    fn syncs_codex_local_access_bucket_when_target_is_custom() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let session_dir = codex_dir.join("sessions");
        fs::create_dir_all(&session_dir).expect("create sessions");
        let session_path = session_dir.join("rollout.jsonl");
        fs::write(
            &session_path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"codex_local_access\"}}\n",
        )
        .expect("write jsonl");

        let conn = Connection::open(codex_dir.join(CODEX_STATE_DB_FILENAME)).expect("open db");
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL);
             INSERT INTO threads (id, model_provider) VALUES ('s1', 'codex_local_access');",
        )
        .expect("seed db");
        drop(conn);

        let outcome = repair_codex_history(RepairOptions {
            codex_dir: codex_dir.clone(),
            target_provider_id: Some("custom".to_string()),
            dry_run: false,
        })
        .expect("repair");

        assert_eq!(outcome.migrated_jsonl_files, 1);
        assert_eq!(outcome.migrated_state_rows, 1);
        assert_eq!(
            outcome.source_provider_ids,
            vec!["codex_local_access".to_string()]
        );
        let session_text = fs::read_to_string(session_path).expect("read jsonl");
        assert!(session_text.contains("\"model_provider\":\"custom\""));

        let conn = Connection::open(codex_dir.join(CODEX_STATE_DB_FILENAME)).expect("open db");
        let provider: String = conn
            .query_row(
                "SELECT model_provider FROM threads WHERE id = 's1'",
                [],
                |row| row.get(0),
            )
            .expect("provider");
        assert_eq!(provider, "custom");
    }

    #[test]
    fn syncs_custom_bucket_when_target_is_openai() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let session_dir = codex_dir.join("sessions");
        fs::create_dir_all(&session_dir).expect("create sessions");
        let session_path = session_dir.join("rollout.jsonl");
        fs::write(
            &session_path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"custom\"}}\n",
        )
        .expect("write jsonl");

        let conn = Connection::open(codex_dir.join(CODEX_STATE_DB_FILENAME)).expect("open db");
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL);
             INSERT INTO threads (id, model_provider) VALUES ('s1', 'custom');",
        )
        .expect("seed db");
        drop(conn);

        let outcome = repair_codex_history(RepairOptions {
            codex_dir: codex_dir.clone(),
            target_provider_id: Some("openai".to_string()),
            dry_run: false,
        })
        .expect("repair");

        assert_eq!(outcome.migrated_jsonl_files, 1);
        assert_eq!(outcome.migrated_state_rows, 1);
        assert_eq!(outcome.source_provider_ids, vec!["custom".to_string()]);
        let session_text = fs::read_to_string(session_path).expect("read jsonl");
        assert!(session_text.contains("\"model_provider\":\"openai\""));

        let conn = Connection::open(codex_dir.join(CODEX_STATE_DB_FILENAME)).expect("open db");
        let provider: String = conn
            .query_row(
                "SELECT model_provider FROM threads WHERE id = 's1'",
                [],
                |row| row.get(0),
            )
            .expect("provider");
        assert_eq!(provider, "openai");
    }

    #[test]
    fn rebuild_preserves_user_renamed_thread_name() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let session_dir = codex_dir.join("sessions");
        fs::create_dir_all(&session_dir).expect("create sessions");
        fs::write(
            session_dir.join("rollout.jsonl"),
            concat!(
                "{\"timestamp\":\"2026-06-06T10:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"cwd\":\"/tmp/project\",\"model_provider\":\"openai\"}}\n",
                "{\"timestamp\":\"2026-06-06T10:01:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"Original prompt\"}}\n"
            ),
        )
        .expect("write jsonl");
        fs::write(
            codex_dir.join("session_index.jsonl"),
            "{\"id\":\"s1\",\"thread_name\":\"Renamed by user\",\"updated_at\":\"2026-06-06T09:00:00Z\"}\n",
        )
        .expect("write index");

        let mut backup = None;
        let changed = rebuild_session_index(&codex_dir, &mut backup, false).expect("rebuild");
        assert_eq!(changed, 1);
        let index_text =
            fs::read_to_string(codex_dir.join("session_index.jsonl")).expect("read index");
        assert!(index_text.contains("\"thread_name\":\"Renamed by user\""));
        assert!(index_text.contains("\"updated_at\":\"2026-06-06T10:01:00Z\""));
    }

    #[test]
    fn rejects_base_url_api_key_provider_import() {
        let options = ProviderImportOptions {
            json_text: Some(
                r#"{
                    "providerName": "Sub2API",
                    "apiBaseUrl": "https://sub2api.example.com/v1/",
                    "OPENAI_API_KEY": "sk-test",
                    "integrationType": "sub2api",
                    "models": [{"model": "gpt-5-codex"}]
                }"#
                .to_string(),
            ),
            ..Default::default()
        };

        let error = parse_provider_import_draft(&options).expect_err("reject");
        assert!(error
            .to_string()
            .contains("only CPA/cockpit/sub2api OpenAI account JSON imports are supported"));
    }

    #[test]
    fn parses_cpa_codex_oauth_json_without_base_url() {
        let draft = parse_provider_import_draft(&ProviderImportOptions {
            json_text: Some(
                r#"{
                    "type": "codex",
                    "account_id": "00000000-0000-4000-9000-000000000000",
                    "chatgpt_account_id": "00000000-0000-4000-9000-000000000000",
                    "email": "mark@example.com",
                    "name": "mark@example.com",
                    "plan_type": "plus",
                    "id_token": "id-token",
                    "access_token": "access-token",
                    "refresh_token": "refresh-token",
                    "session_token": "session-token",
                    "last_refresh": "2026-06-06T09:11:35.028Z",
                    "expired": "2026-08-06T14:29:36.155Z"
                }"#
                .to_string(),
            ),
            ..Default::default()
        })
        .expect("parse cpa");

        assert_eq!(draft.import_kind, "openai_account");
        assert_eq!(
            draft.oauth_account_id.as_deref(),
            Some("00000000-0000-4000-9000-000000000000")
        );
        assert_eq!(draft.provider_name, "mark@example.com");
        assert!(draft.base_url.is_none());
        assert!(draft.settings_config["auth"]["OPENAI_API_KEY"].is_null());
        assert_eq!(
            draft.settings_config["auth"]["tokens"]["access_token"].as_str(),
            Some("access-token")
        );
        assert_eq!(draft.settings_config["config"].as_str(), Some(""));
        assert_eq!(draft.meta["endpointAutoSelect"].as_bool(), Some(true));
        assert!(draft.meta.get("providerType").is_none());
    }

    #[test]
    fn parses_sub2api_oauth_export_account() {
        let draft = parse_provider_import_draft(&ProviderImportOptions {
            json_text: Some(
                r#"{
                    "providerName": "Sub2API Export",
                    "exported_at": "2026-06-06T09:11:53.691Z",
                    "accounts": [{
                        "name": "mark@example.com",
                        "platform": "openai",
                        "type": "oauth",
                        "credentials": {
                            "access_token": "access-token",
                            "account_id": "account-id",
                            "chatgpt_account_id": "chatgpt-account-id",
                            "id_token": "id-token",
                            "email": "mark@example.com",
                            "plan_type": "plus"
                        },
                        "extra": {
                            "last_refresh": "2026-06-06T09:11:35.028Z"
                        }
                    }]
                }"#
                .to_string(),
            ),
            ..Default::default()
        })
        .expect("parse sub2api oauth");

        assert_eq!(draft.import_kind, "openai_account");
        assert_eq!(
            draft.oauth_account_id.as_deref(),
            Some("chatgpt-account-id")
        );
        assert_eq!(draft.provider_name, "mark@example.com");
        assert_eq!(
            draft.settings_config["auth"]["tokens"]["email"].as_str(),
            Some("mark@example.com")
        );
    }

    #[test]
    fn provider_name_defaults_to_imported_account_name() {
        let draft = parse_provider_import_draft(&ProviderImportOptions {
            json_text: Some(
                r#"{
                    "name": "outer-export-name",
                    "accounts": [{
                        "name": "account@example.com",
                        "platform": "openai",
                        "type": "oauth",
                        "credentials": {
                            "access_token": "access-token",
                            "chatgpt_account_id": "account-id",
                            "email": "account@example.com"
                        }
                    }]
                }"#
                .to_string(),
            ),
            ..Default::default()
        })
        .expect("parse account name");

        assert_eq!(draft.provider_name, "account@example.com");
    }

    #[test]
    fn imports_openai_account_provider_without_switching_current() {
        let dir = tempdir().expect("tempdir");
        let cc_switch_dir = dir.path().join(".cc-switch");
        fs::create_dir_all(&cc_switch_dir).expect("create dir");
        let db_path = cc_switch_dir.join(CC_SWITCH_DB_FILENAME);
        let conn = Connection::open(&db_path).expect("open db");
        conn.execute_batch(
            "CREATE TABLE providers (
                id TEXT NOT NULL,
                app_type TEXT NOT NULL,
                name TEXT NOT NULL,
                settings_config TEXT NOT NULL,
                website_url TEXT,
                category TEXT,
                created_at INTEGER,
                sort_index INTEGER,
                notes TEXT,
                icon TEXT,
                icon_color TEXT,
                meta TEXT NOT NULL DEFAULT '{}',
                is_current BOOLEAN NOT NULL DEFAULT 0,
                in_failover_queue BOOLEAN NOT NULL DEFAULT 0,
                PRIMARY KEY (id, app_type)
            );
            INSERT INTO providers (id, app_type, name, settings_config, meta, is_current)
            VALUES ('current', 'codex', 'Current', '{\"auth\":{},\"config\":\"model_provider = \\\"current\\\"\"}', '{}', 1);",
        )
        .expect("schema");
        drop(conn);

        let outcome = import_cc_switch_provider(ProviderImportOptions {
            cc_switch_dir: cc_switch_dir.clone(),
            json_text: Some(
                r#"{
                    "type": "codex",
                    "chatgpt_account_id": "chatgpt-account-id",
                    "email": "mark@example.com",
                    "access_token": "access-token",
                    "refresh_token": "refresh-token"
                }"#
                .to_string(),
            ),
            ..Default::default()
        })
        .expect("import openai account");

        assert!(outcome.created);
        assert_eq!(outcome.import_kind, "openai_account");
        assert_eq!(
            outcome.oauth_account_id.as_deref(),
            Some("chatgpt-account-id")
        );
        assert!(outcome.oauth_store_path.is_none());

        let conn = Connection::open(&db_path).expect("open db");
        let current_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM providers WHERE app_type = 'codex' AND id = 'current' AND is_current = 1",
                [],
                |row| row.get(0),
            )
            .expect("current count");
        assert_eq!(current_count, 1);
        let category: String = conn
            .query_row(
                "SELECT category FROM providers WHERE app_type = 'codex' AND id = ?",
                [outcome.provider_id.as_str()],
                |row| row.get(0),
            )
            .expect("category");
        assert_eq!(category, "official");
        let (settings_text, meta_text): (String, String) = conn
            .query_row(
                "SELECT settings_config, meta FROM providers WHERE app_type = 'codex' AND id = ?",
                [outcome.provider_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("settings");
        let settings: Value = serde_json::from_str(&settings_text).expect("settings json");
        let meta: Value = serde_json::from_str(&meta_text).expect("meta json");
        assert_eq!(settings["config"].as_str(), Some(""));
        assert_eq!(
            settings["auth"]["tokens"]["refresh_token"].as_str(),
            Some("refresh-token")
        );
        assert!(meta.get("providerType").is_none());
    }

    #[test]
    fn rejects_manual_provider_without_switching_current() {
        let dir = tempdir().expect("tempdir");
        let cc_switch_dir = dir.path().join(".cc-switch");
        fs::create_dir_all(&cc_switch_dir).expect("create dir");
        let db_path = cc_switch_dir.join(CC_SWITCH_DB_FILENAME);
        let conn = Connection::open(&db_path).expect("open db");
        conn.execute_batch(
            "CREATE TABLE providers (
                id TEXT NOT NULL,
                app_type TEXT NOT NULL,
                name TEXT NOT NULL,
                settings_config TEXT NOT NULL,
                website_url TEXT,
                category TEXT,
                created_at INTEGER,
                sort_index INTEGER,
                notes TEXT,
                icon TEXT,
                icon_color TEXT,
                meta TEXT NOT NULL DEFAULT '{}',
                is_current BOOLEAN NOT NULL DEFAULT 0,
                in_failover_queue BOOLEAN NOT NULL DEFAULT 0,
                PRIMARY KEY (id, app_type)
            );
            INSERT INTO providers (id, app_type, name, settings_config, meta, is_current)
            VALUES ('current', 'codex', 'Current', '{\"auth\":{},\"config\":\"model_provider = \\\"current\\\"\"}', '{}', 1);",
        )
        .expect("schema");
        drop(conn);

        let error = import_cc_switch_provider(ProviderImportOptions {
            cc_switch_dir,
            base_url: Some("https://newapi.example.com/v1".to_string()),
            api_key: Some("sk-import".to_string()),
            provider_name: Some("NewAPI".to_string()),
            ..Default::default()
        })
        .expect_err("reject");
        assert!(error
            .to_string()
            .contains("only CPA/cockpit/sub2api OpenAI account JSON imports are supported"));

        let conn = Connection::open(&db_path).expect("open db");
        let current_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM providers WHERE app_type = 'codex' AND id = 'current' AND is_current = 1",
                [],
                |row| row.get(0),
            )
            .expect("current count");
        let provider_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM providers WHERE app_type = 'codex'",
                [],
                |row| row.get(0),
            )
            .expect("provider count");
        assert_eq!(current_count, 1);
        assert_eq!(provider_count, 1);
    }
}
