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
    "https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/features/acp.md";
pub(crate) const HERMES_ACP_BINARY: &str = "hermes-acp";
pub(crate) const HERMES_BINARY: &str = "hermes";

const HERMES_MODE_DEFAULT: &str = "default";
const HERMES_MODE_ACCEPT_EDITS: &str = "accept_edits";
const HERMES_MODE_DONT_ASK: &str = "dont_ask";

#[derive(Debug)]
pub(crate) struct HermesAcpLaunch {
    pub command: PathBuf,
    pub args: Vec<String>,
}

pub struct HermesAcpProvider;

impl goose_providers::base::ProviderDescriptor for HermesAcpProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::new(
            HERMES_ACP_PROVIDER_NAME,
            "Hermes ACP",
            "Use goose with Hermes Agent via ACP over stdio. Requires an existing Hermes install; goose does not install Hermes or modify ~/.hermes.",
            ACP_CURRENT_MODEL,
            vec![],
            HERMES_ACP_DOC_URL,
            vec![],
        )
        .with_setup_steps(vec![
            "Ensure `hermes-acp` or `hermes` is already on your PATH (goose does not install Hermes)",
            "Authenticate Hermes with `hermes model` if needed. goose does not modify ~/.hermes",
        ])
        .with_setup(
            ProviderSetupMetadata::cli_agent(
                HERMES_ACP_BINARY,
                &["hermes-acp", "hermes"],
            )
            .with_acp()
            .with_docs_url(HERMES_ACP_DOC_URL),
        )
        .with_model_selection_hint("Use the Hermes CLI (`hermes model`) to configure models")
    }
}

pub(crate) fn resolve_hermes_acp_launch() -> Result<HermesAcpLaunch> {
    resolve_hermes_acp_launch_with(|name| SearchPaths::builder().resolve(name))
}

fn hermes_mode_mapping() -> HashMap<GooseMode, Vec<String>> {
    HashMap::from([
        // Closest advertised autonomous mode; still guards sensitive paths.
        (GooseMode::Auto, vec![HERMES_MODE_DONT_ASK.to_string()]),
        (GooseMode::Approve, vec![HERMES_MODE_DEFAULT.to_string()]),
        // Auto-allows workspace and /tmp edits; still asks for sensitive paths.
        (
            GooseMode::SmartApprove,
            vec![HERMES_MODE_ACCEPT_EDITS.to_string()],
        ),
        // Hermes has no plan mode; fail closed to ask-before-edits.
        (GooseMode::Chat, vec![HERMES_MODE_DEFAULT.to_string()]),
    ])
}

fn resolve_hermes_acp_launch_with(
    resolve: impl Fn(&str) -> Result<PathBuf>,
) -> Result<HermesAcpLaunch> {
    if let Ok(command) = resolve(HERMES_ACP_BINARY) {
        return Ok(HermesAcpLaunch {
            command,
            args: vec![],
        });
    }
    if let Ok(command) = resolve(HERMES_BINARY) {
        return Ok(HermesAcpLaunch {
            command,
            args: vec!["acp".to_string()],
        });
    }
    anyhow::bail!("could not resolve command 'hermes-acp' or 'hermes': file does not exist")
}

impl HermesAcpProvider {
    fn create(
        extensions: Vec<crate::config::ExtensionConfig>,
        working_dir: PathBuf,
        use_default_model: bool,
    ) -> BoxFuture<'static, Result<AcpProvider>> {
        Box::pin(async move {
            let config = Config::global();
            let launch = resolve_hermes_acp_launch()?;
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

            let mode_mapping = hermes_mode_mapping();
            let provider_config = AcpProviderConfig {
                command: launch.command,
                args: launch.args,
                env: vec![],
                env_remove: vec![],
                work_dir: working_dir,
                mcp_servers: extension_configs_to_mcp_servers(&extensions),
                session_mode_id: mode_mapping[&goose_mode].first().cloned(),
                session_config_options,
                model_config_option_id: Some("model".to_string()),
                mode_mapping,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::catalog::{ProviderSetupCategory, ProviderSetupMethod};

    #[test]
    fn metadata_registers_hermes_acp_as_agent_acp_provider() {
        let metadata = HermesAcpProvider::metadata();
        assert_eq!(metadata.name, HERMES_ACP_PROVIDER_NAME);
        assert_eq!(metadata.display_name, "Hermes ACP");
        assert_eq!(metadata.default_model, ACP_CURRENT_MODEL);

        let setup = metadata
            .setup
            .expect("hermes-acp should expose setup metadata");
        assert_eq!(setup.category, ProviderSetupCategory::Agent);
        assert!(setup.acp);
        assert_eq!(setup.setup_method, ProviderSetupMethod::CliAuth);
        assert_eq!(setup.binary_name.as_deref(), Some(HERMES_ACP_BINARY));
        assert!(!setup.setup_capabilities.install);
        assert!(setup.aliases.iter().any(|alias| alias == "hermes-acp"));
        assert!(setup.aliases.iter().any(|alias| alias == "hermes"));
    }

    #[test]
    fn prefers_hermes_acp_binary_over_hermes() {
        let launch = resolve_hermes_acp_launch_with(|name| match name {
            HERMES_ACP_BINARY => Ok(PathBuf::from("/tmp/hermes-acp")),
            HERMES_BINARY => Ok(PathBuf::from("/tmp/hermes")),
            other => anyhow::bail!("missing {other}"),
        })
        .unwrap();

        assert_eq!(launch.command, PathBuf::from("/tmp/hermes-acp"));
        assert!(launch.args.is_empty());
    }

    #[test]
    fn falls_back_to_hermes_with_acp_arg() {
        let launch = resolve_hermes_acp_launch_with(|name| match name {
            HERMES_BINARY => Ok(PathBuf::from("/usr/bin/hermes")),
            other => anyhow::bail!("missing {other}"),
        })
        .unwrap();

        assert_eq!(launch.command, PathBuf::from("/usr/bin/hermes"));
        assert_eq!(launch.args, vec!["acp".to_string()]);
    }

    #[test]
    fn errors_when_neither_binary_exists() {
        let error = resolve_hermes_acp_launch_with(|_| anyhow::bail!("missing")).unwrap_err();
        let message = error.to_string();
        assert!(message.contains(HERMES_ACP_BINARY), "{message}");
        assert!(message.contains(HERMES_BINARY), "{message}");
    }

    #[test]
    fn maps_goose_modes_to_advertised_hermes_session_modes() {
        let mapping = hermes_mode_mapping();
        assert_eq!(mapping[&GooseMode::Auto], vec![HERMES_MODE_DONT_ASK]);
        assert_eq!(mapping[&GooseMode::Approve], vec![HERMES_MODE_DEFAULT]);
        assert_eq!(
            mapping[&GooseMode::SmartApprove],
            vec![HERMES_MODE_ACCEPT_EDITS]
        );
        assert_eq!(mapping[&GooseMode::Chat], vec![HERMES_MODE_DEFAULT]);

        let advertised = [
            HERMES_MODE_DEFAULT,
            HERMES_MODE_ACCEPT_EDITS,
            HERMES_MODE_DONT_ASK,
        ];
        for ids in mapping.values() {
            assert!(!ids.is_empty());
            assert!(
                ids.iter().all(|id| advertised.contains(&id.as_str())),
                "unexpected Hermes mode id in {ids:?}"
            );
        }
    }

    #[test]
    fn session_mode_id_follows_mapped_goose_mode() {
        let mapping = hermes_mode_mapping();
        let session_mode_id = |mode| mapping[&mode].first().cloned();
        assert_eq!(
            session_mode_id(GooseMode::Auto).as_deref(),
            Some(HERMES_MODE_DONT_ASK)
        );
        assert_eq!(
            session_mode_id(GooseMode::Approve).as_deref(),
            Some(HERMES_MODE_DEFAULT)
        );
        assert_eq!(
            session_mode_id(GooseMode::SmartApprove).as_deref(),
            Some(HERMES_MODE_ACCEPT_EDITS)
        );
        assert_eq!(
            session_mode_id(GooseMode::Chat).as_deref(),
            Some(HERMES_MODE_DEFAULT)
        );
    }
}
