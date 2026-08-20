use anyhow::Result;
use futures::future::BoxFuture;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::acp::{
    configured_model_for_provider, extension_configs_to_mcp_servers, AcpProvider,
    AcpProviderConfig, ACP_CURRENT_MODEL,
};
use crate::config::search_path::SearchPaths;
use crate::config::{Config, GooseMode};
use crate::providers::base::{
    current_working_dir, ProviderDef, ProviderDescriptor, ProviderMetadata,
};
use crate::providers::catalog::ProviderSetupMetadata;

pub(crate) const HERMES_ACP_PROVIDER_NAME: &str = "hermes-acp";
const HERMES_ACP_DOC_URL: &str =
    "https://hermes-agent.nousresearch.com/docs/user-guide/features/acp";
pub(crate) const HERMES_ACP_BINARY: &str = "hermes-acp";
pub(crate) const HERMES_ACP_FALLBACK_BINARY: &str = "hermes";

pub struct HermesAcpProvider;

impl goose_providers::base::ProviderDescriptor for HermesAcpProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::new(
            HERMES_ACP_PROVIDER_NAME,
            "Hermes Agent",
            "Use goose with Hermes Agent via ACP (`hermes-acp`, or `hermes acp`).",
            ACP_CURRENT_MODEL,
            vec![],
            HERMES_ACP_DOC_URL,
            vec![],
        )
        .with_setup_steps(vec![
            "Install Hermes Agent so `hermes-acp` or `hermes` is on PATH (often ~/.local/bin)",
            "Authenticate with the Hermes CLI (`hermes model` or `hermes acp --setup`). Goose does not write ~/.hermes.",
        ])
        .with_setup(
            ProviderSetupMetadata::cli_agent(
                HERMES_ACP_BINARY,
                &["hermes-acp", "hermes"],
            )
            .with_acp()
            .with_docs_url(HERMES_ACP_DOC_URL)
            .show_only_when_installed(),
        )
        .with_model_selection_hint("Use the Hermes CLI to configure models")
    }
}

impl HermesAcpProvider {
    fn resolve_command() -> Result<(PathBuf, Vec<String>)> {
        if let Ok(command) = SearchPaths::builder().with_npm().resolve(HERMES_ACP_BINARY) {
            return Ok((command, vec![]));
        }
        let command = SearchPaths::builder()
            .with_npm()
            .resolve(HERMES_ACP_FALLBACK_BINARY)?;
        Ok((command, vec!["acp".to_string()]))
    }

    fn create(
        extensions: Vec<crate::config::ExtensionConfig>,
        working_dir: PathBuf,
        use_default_model: bool,
    ) -> BoxFuture<'static, Result<AcpProvider>> {
        Box::pin(async move {
            let config = Config::global();
            let (resolved_command, args) = Self::resolve_command()?;
            let goose_mode = config.get_goose_mode().unwrap_or(GooseMode::Auto);
            let model = if use_default_model {
                ACP_CURRENT_MODEL.to_string()
            } else {
                configured_model_for_provider(config, HERMES_ACP_PROVIDER_NAME)
            };

            let session_config_options = if model == ACP_CURRENT_MODEL {
                vec![]
            } else {
                vec![("model".to_string(), model)]
            };

            let provider_config = AcpProviderConfig {
                command: resolved_command,
                args,
                env: vec![],
                env_remove: vec![],
                work_dir: working_dir,
                mcp_servers: extension_configs_to_mcp_servers(&extensions),
                session_mode_id: None,
                session_config_options,
                model_config_option_id: Some("model".to_string()),
                mode_mapping: HashMap::new(),
                notification_callback: None,
            };

            let metadata = Self::metadata();
            AcpProvider::connect(metadata.name, goose_mode, provider_config).await
        })
    }
}

impl ProviderDef for HermesAcpProvider {
    type Provider = AcpProvider;

    fn from_env(
        extensions: Vec<crate::config::ExtensionConfig>,
        tls_config: Option<crate::providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<AcpProvider>> {
        Self::from_env_with_working_dir(extensions, current_working_dir(), tls_config)
    }

    fn from_env_with_working_dir(
        extensions: Vec<crate::config::ExtensionConfig>,
        working_dir: PathBuf,
        _tls_config: Option<crate::providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<AcpProvider>> {
        Self::create(extensions, working_dir, false)
    }

    fn from_env_with_default_model(
        extensions: Vec<crate::config::ExtensionConfig>,
        _tls_config: Option<crate::providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<AcpProvider>> {
        Self::create(extensions, current_working_dir(), true)
    }
}
