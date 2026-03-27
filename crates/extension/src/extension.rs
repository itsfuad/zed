mod capabilities;
pub mod extension_builder;
mod extension_events;
mod extension_host_proxy;
mod extension_manifest;
mod types;

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use ::lsp::LanguageServerName;
use anyhow::{Context as _, Result, bail};
use async_trait::async_trait;
use gpui::{App, Task};
use language::LanguageName;
use semver::Version;
use task::{SpawnInTerminal, ZedDebugConfig};
use util::rel_path::RelPath;

pub use crate::capabilities::*;
pub use crate::extension_events::*;
pub use crate::extension_host_proxy::*;
pub use crate::extension_manifest::*;
pub use crate::types::*;

/// Initializes the `extension` crate.
pub fn init(cx: &mut App) {
    extension_events::init(cx);
    ExtensionHostProxy::default_global(cx);
}

#[async_trait]
pub trait WorktreeDelegate: Send + Sync + 'static {
    fn id(&self) -> u64;
    fn root_path(&self) -> String;
    async fn read_text_file(&self, path: &RelPath) -> Result<String>;
    async fn which(&self, binary_name: String) -> Option<String>;
    async fn shell_env(&self) -> Vec<(String, String)>;
}

pub trait ProjectDelegate: Send + Sync + 'static {
    fn worktree_ids(&self) -> Vec<u64>;
}

pub trait KeyValueStoreDelegate: Send + Sync + 'static {
    fn insert(&self, key: String, docs: String) -> Task<Result<()>>;
}

#[async_trait]
pub trait Extension: Send + Sync + 'static {
    /// Returns the [`ExtensionManifest`] for this extension.
    fn manifest(&self) -> Arc<ExtensionManifest>;

    /// Returns the path to this extension's working directory.
    fn work_dir(&self) -> Arc<Path>;

    /// Returns a path relative to this extension's working directory.
    fn path_from_extension(&self, path: &Path) -> PathBuf {
        util::normalize_path(&self.work_dir().join(path))
    }

    async fn language_server_command(
        &self,
        language_server_id: LanguageServerName,
        language_name: LanguageName,
        worktree: Arc<dyn WorktreeDelegate>,
    ) -> Result<Command>;

    async fn language_server_initialization_options(
        &self,
        language_server_id: LanguageServerName,
        language_name: LanguageName,
        worktree: Arc<dyn WorktreeDelegate>,
    ) -> Result<Option<String>>;

    async fn language_server_workspace_configuration(
        &self,
        language_server_id: LanguageServerName,
        worktree: Arc<dyn WorktreeDelegate>,
    ) -> Result<Option<String>>;

    async fn language_server_initialization_options_schema(
        &self,
        language_server_id: LanguageServerName,
        worktree: Arc<dyn WorktreeDelegate>,
    ) -> Result<Option<String>>;

    async fn language_server_workspace_configuration_schema(
        &self,
        language_server_id: LanguageServerName,
        worktree: Arc<dyn WorktreeDelegate>,
    ) -> Result<Option<String>>;

    async fn language_server_additional_initialization_options(
        &self,
        language_server_id: LanguageServerName,
        target_language_server_id: LanguageServerName,
        worktree: Arc<dyn WorktreeDelegate>,
    ) -> Result<Option<String>>;

    async fn language_server_additional_workspace_configuration(
        &self,
        language_server_id: LanguageServerName,
        target_language_server_id: LanguageServerName,
        worktree: Arc<dyn WorktreeDelegate>,
    ) -> Result<Option<String>>;

    async fn labels_for_completions(
        &self,
        language_server_id: LanguageServerName,
        completions: Vec<Completion>,
    ) -> Result<Vec<Option<CodeLabel>>>;

    async fn labels_for_symbols(
        &self,
        language_server_id: LanguageServerName,
        symbols: Vec<Symbol>,
    ) -> Result<Vec<Option<CodeLabel>>>;

    async fn complete_slash_command_argument(
        &self,
        command: SlashCommand,
        arguments: Vec<String>,
    ) -> Result<Vec<SlashCommandArgumentCompletion>>;

    async fn run_slash_command(
        &self,
        command: SlashCommand,
        arguments: Vec<String>,
        worktree: Option<Arc<dyn WorktreeDelegate>>,
    ) -> Result<SlashCommandOutput>;

    async fn context_server_command(
        &self,
        context_server_id: Arc<str>,
        project: Arc<dyn ProjectDelegate>,
    ) -> Result<Command>;

    async fn context_server_configuration(
        &self,
        context_server_id: Arc<str>,
        project: Arc<dyn ProjectDelegate>,
    ) -> Result<Option<ContextServerConfiguration>>;

    async fn suggest_docs_packages(&self, provider: Arc<str>) -> Result<Vec<String>>;

    async fn index_docs(
        &self,
        provider: Arc<str>,
        package_name: Arc<str>,
        kv_store: Arc<dyn KeyValueStoreDelegate>,
    ) -> Result<()>;

    async fn get_dap_binary(
        &self,
        dap_name: Arc<str>,
        config: DebugTaskDefinition,
        user_installed_path: Option<PathBuf>,
        worktree: Arc<dyn WorktreeDelegate>,
    ) -> Result<DebugAdapterBinary>;

    async fn dap_request_kind(
        &self,
        dap_name: Arc<str>,
        config: serde_json::Value,
    ) -> Result<StartDebuggingRequestArgumentsRequest>;

    async fn dap_config_to_scenario(&self, config: ZedDebugConfig) -> Result<DebugScenario>;

    async fn dap_locator_create_scenario(
        &self,
        locator_name: String,
        build_config_template: BuildTaskTemplate,
        resolved_label: String,
        debug_adapter_name: String,
    ) -> Result<Option<DebugScenario>>;
    async fn run_dap_locator(
        &self,
        locator_name: String,
        config: SpawnInTerminal,
    ) -> Result<DebugRequest>;
}

pub fn parse_wasm_extension_version(extension_id: &str, wasm_bytes: &[u8]) -> Result<Version> {
    let mut version = None;

    for part in wasmparser::Parser::new(0).parse_all(wasm_bytes) {
        if let wasmparser::Payload::CustomSection(s) =
            part.context("error parsing wasm extension")?
            && s.name() == "zed:api-version"
        {
            version = parse_wasm_extension_version_custom_section(s.data());
            if version.is_none() {
                bail!(
                    "extension {} has invalid zed:api-version section: {:?}",
                    extension_id,
                    s.data()
                );
            }
        }
    }

    // The reason we wait until we're done parsing all of the Wasm bytes to return the version
    // is to work around a panic that can happen inside of Wasmtime when the bytes are invalid.
    //
    // By parsing the entirety of the Wasm bytes before we return, we're able to detect this problem
    // earlier as an `Err` rather than as a panic.
    version.with_context(|| format!("extension {extension_id} has no zed:api-version section"))
}

fn parse_wasm_extension_version_custom_section(data: &[u8]) -> Option<Version> {
    if data.len() == 6 {
        Some(Version::new(
            u16::from_be_bytes([data[0], data[1]]) as _,
            u16::from_be_bytes([data[2], data[3]]) as _,
            u16::from_be_bytes([data[4], data[5]]) as _,
        ))
    } else {
        None
    }
}

pub fn resolve_language_icon(language_path: &Path, icon: Option<&str>) -> Option<Arc<str>> {
    let icon = icon?;
    if !looks_like_icon_asset_path(icon) {
        return Some(icon.into());
    }

    let icon_path = Path::new(icon);
    if !is_safe_relative_icon_path(icon_path) {
        return None;
    }

    let resolved_icon_path = language_path.join(icon_path);
    let canonical_language_path = std::fs::canonicalize(language_path).ok()?;
    let canonical_icon_path = std::fs::canonicalize(&resolved_icon_path).ok()?;
    if !canonical_icon_path.starts_with(&canonical_language_path) {
        return None;
    }

    Some(resolved_icon_path.to_string_lossy().into_owned().into())
}

fn looks_like_icon_asset_path(icon: &str) -> bool {
    icon.contains(['/', '\\']) || icon.ends_with(".svg")
}

fn is_safe_relative_icon_path(icon_path: &Path) -> bool {
    icon_path
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::resolve_language_icon;

    #[test]
    fn resolves_language_icon_keys_without_rewriting() {
        assert_eq!(
            resolve_language_icon(Path::new("languages/example"), Some("json")).as_deref(),
            Some("json")
        );
    }

    #[test]
    fn resolves_language_icon_assets_relative_to_language_dir() {
        let temp_dir = TempDir::new().unwrap();
        let language_dir = temp_dir.path().join("languages").join("example");
        fs::create_dir_all(&language_dir).unwrap();
        fs::write(language_dir.join("language-icon.svg"), "").unwrap();

        let expected = language_dir
            .join("language-icon.svg")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            resolve_language_icon(&language_dir, Some("language-icon.svg")).as_deref(),
            Some(expected.as_str())
        );
    }

    #[test]
    fn resolves_nested_language_icon_assets_within_language_dir() {
        let temp_dir = TempDir::new().unwrap();
        let language_dir = temp_dir.path().join("languages").join("example");
        let icons_dir = language_dir.join("icons");
        fs::create_dir_all(&icons_dir).unwrap();
        fs::write(icons_dir.join("language-icon.svg"), "").unwrap();

        let expected = language_dir
            .join(Path::new("icons/language-icon.svg"))
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            resolve_language_icon(&language_dir, Some("icons/language-icon.svg")).as_deref(),
            Some(expected.as_str())
        );
    }

    #[test]
    fn rejects_language_icon_paths_with_parent_dir_components() {
        let temp_dir = TempDir::new().unwrap();
        let language_dir = temp_dir.path().join("languages").join("example");
        fs::create_dir_all(&language_dir).unwrap();

        assert_eq!(
            resolve_language_icon(&language_dir, Some("../language-icon.svg")).as_deref(),
            None
        );
    }

    #[test]
    fn rejects_absolute_language_icon_paths() {
        let temp_dir = TempDir::new().unwrap();
        let language_dir = temp_dir.path().join("languages").join("example");
        fs::create_dir_all(&language_dir).unwrap();

        let absolute_icon_path = temp_dir.path().join("language-icon.svg");
        fs::write(&absolute_icon_path, "").unwrap();

        assert_eq!(
            resolve_language_icon(&language_dir, Some(&absolute_icon_path.to_string_lossy()))
                .as_deref(),
            None
        );
    }
}
