use super::*;
use crate::{
    ai::request_usage_model::{RequestLimitInfo, RequestLimitRefreshDuration},
    test_util::settings::initialize_settings_for_tests,
};
use chrono::Utc;
use std::collections::HashMap;
use warp_graphql::scalars::time::ServerTimestamp;
use warpui::{App, SingletonEntity};

fn create_test_request_limit_info(
    limit: usize,
    used: usize,
    next_refresh: DateTime<Utc>,
    is_unlimited: bool,
    refresh_duration: RequestLimitRefreshDuration,
) -> RequestLimitInfo {
    RequestLimitInfo {
        limit,
        num_requests_used_since_refresh: used,
        next_refresh_time: ServerTimestamp::new(next_refresh),
        is_unlimited,
        request_limit_refresh_duration: refresh_duration,
        is_unlimited_voice: false,
        voice_request_limit: 0,
        voice_requests_used_since_last_refresh: 0,
        is_unlimited_codebase_indices: false,
        max_codebase_indices: 0,
        max_files_per_repo: 5000,
        embedding_generation_batch_size: 100,
    }
}

// FocusedTerminalInfo Tests

#[test]
fn test_update_both_values_changed() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        let model_handle_clone = model_handle.clone();
        model_handle.update(&mut app, move |_, ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle_clone,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // Update both values to (true, false)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, false, ctx);
        });

        // Verify model state
        model_handle.read(&app, |model, _| {
            assert!(model.contains_any_remote_blocks());
            assert!(!model.contains_any_restored_remote_blocks());
        });

        // Verify event was emitted exactly once
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1);
    });
}

#[test]
fn test_update_additional_value_changed() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        let model_handle_clone = model_handle.clone();
        model_handle.update(&mut app, move |_, ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle_clone,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // First update to (true, false)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, false, ctx);
        });

        // Clear events by draining the channel
        while receiver.try_recv().is_ok() {}

        // Now update to (true, true) - only changing restored blocks
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Verify model state
        model_handle.read(&app, |model, _| {
            assert!(model.contains_any_remote_blocks());
            assert!(model.contains_any_restored_remote_blocks());
        });

        // Verify event was emitted exactly once
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1);
    });
}

#[test]
fn test_update_no_change() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        let model_handle_clone = model_handle.clone();
        model_handle.update(&mut app, move |_, ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle_clone,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // First update to (true, true)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Clear events by draining the channel
        while receiver.try_recv().is_ok() {}

        // Update with same values (true, true)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Verify model state remains the same
        model_handle.read(&app, |model, _| {
            assert!(model.contains_any_remote_blocks());
            assert!(model.contains_any_restored_remote_blocks());
        });

        // Verify no event was emitted
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 0);
    });
}

#[test]
fn test_update_only_remote_toggles() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        let model_handle_clone = model_handle.clone();
        model_handle.update(&mut app, move |_, ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle_clone,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // First update to (true, true)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Clear events by draining the channel
        while receiver.try_recv().is_ok() {}

        // Update with (false, true) - only remote blocks changes
        model_handle.update(&mut app, |model, ctx| {
            model.update(false, true, ctx);
        });

        // Verify model state
        model_handle.read(&app, |model, _| {
            assert!(!model.contains_any_remote_blocks());
            assert!(model.contains_any_restored_remote_blocks());
        });

        // Verify event was emitted exactly once
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1);
    });
}

#[test]
fn test_update_only_restored_toggles() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        let model_handle_clone = model_handle.clone();
        model_handle.update(&mut app, move |_, ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle_clone,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // First update to (true, true)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Clear events by draining the channel
        while receiver.try_recv().is_ok() {}

        // Update with (true, false) - only restored blocks changes
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, false, ctx);
        });

        // Verify model state
        model_handle.read(&app, |model, _| {
            assert!(model.contains_any_remote_blocks());
            assert!(!model.contains_any_restored_remote_blocks());
        });

        // Verify event was emitted exactly once
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1);
    });
}

// ToolbarCommandMap Tests

#[test]
fn test_toolbar_command_map_deserialize_from_map() {
    let json = serde_json::json!({
        "^claude": "Claude",
        "^gemini": "Gemini",
        "^codex": ""
    });
    let map: ToolbarCommandMap = serde_json::from_value(json).unwrap();
    assert_eq!(map.0.len(), 3);
    assert_eq!(map.0["^claude"], "Claude");
    assert_eq!(map.0["^gemini"], "Gemini");
    assert_eq!(map.0["^codex"], "");
}

#[test]
fn test_toolbar_command_map_deserialize_from_legacy_vec() {
    let json = serde_json::json!(["^claude", "^gemini", "^custom"]);
    let map: ToolbarCommandMap = serde_json::from_value(json).unwrap();
    assert_eq!(map.0.len(), 3);
    // Legacy vec format should assign empty agent values.
    for (_, agent) in map.0.iter() {
        assert_eq!(agent, "");
    }
    let keys: Vec<_> = map.0.keys().collect();
    assert_eq!(keys, vec!["^claude", "^gemini", "^custom"]);
}

#[test]
fn test_toolbar_command_map_from_file_value_map_format() {
    use settings_value::SettingsValue;

    let value = serde_json::json!({
        "^claude": "Claude",
        "^amp": "Amp"
    });
    let map = ToolbarCommandMap::from_file_value(&value).unwrap();
    assert_eq!(map.0.len(), 2);
    assert_eq!(map.0["^claude"], "Claude");
    assert_eq!(map.0["^amp"], "Amp");
}

#[test]
fn test_toolbar_command_map_from_file_value_legacy_array() {
    use settings_value::SettingsValue;

    // Patterns are intentionally non-alphabetical to verify insertion order is preserved.
    let value = serde_json::json!(["^zebra", "^alpha", "^middle"]);
    let map = ToolbarCommandMap::from_file_value(&value).unwrap();
    assert_eq!(map.0.len(), 3);
    assert_eq!(map.0["^zebra"], "");
    assert_eq!(map.0["^alpha"], "");
    assert_eq!(map.0["^middle"], "");
    let keys: Vec<_> = map.0.keys().collect();
    assert_eq!(keys, vec!["^zebra", "^alpha", "^middle"]);
}

#[test]
fn test_toolbar_command_map_from_file_value_invalid() {
    use settings_value::SettingsValue;

    let value = serde_json::json!(42);
    assert!(ToolbarCommandMap::from_file_value(&value).is_none());
}

#[test]
fn test_toolbar_command_map_roundtrip() {
    use settings_value::SettingsValue;

    let mut inner = IndexMap::new();
    inner.insert("^claude".to_string(), "Claude".to_string());
    inner.insert("^custom".to_string(), String::new());
    let original = ToolbarCommandMap::new(inner);

    let file_value = original.to_file_value();
    let restored = ToolbarCommandMap::from_file_value(&file_value).unwrap();
    assert_eq!(original, restored);
}

#[test]
fn test_acp_agent_registry_contains_seed_agents() {
    let opencode = KNOWN_ACP_AGENTS
        .iter()
        .find(|entry| entry.registry_key == "opencode")
        .expect("opencode ACP registry entry exists");
    assert_eq!(opencode.command.command, "opencode");
    assert_eq!(opencode.command.args, &["acp", "--port", "0"]);

    let codex = KNOWN_ACP_AGENTS
        .iter()
        .find(|entry| entry.registry_key == "codex-acp")
        .expect("codex ACP registry entry exists");
    assert_eq!(codex.command.command, "codex-acp");
    assert_eq!(codex.command.args, &[] as &[&str]);
    assert_eq!(
        codex.fallback_command,
        Some(AcpAgentCommandTemplate {
            command: "npx",
            args: &["-y", "@zed-industries/codex-acp"],
        })
    );
}

#[test]
fn test_acp_agent_config_roundtrip() {
    use settings_value::SettingsValue;

    let original = AcpAgentConfig {
        id: AcpAgentId::new("opencode-local"),
        name: "OpenCode".to_string(),
        command: "opencode".to_string(),
        transport: AcpAgentTransportConfig::Local,
        args: vec!["acp".to_string()],
        env: vec![AcpAgentEnvVar {
            name: "OPENCODE_CONFIG".to_string(),
            value: AcpAgentEnvValue::SecretRef {
                key: "opencode-config".to_string(),
            },
        }],
        mcp_allowlist: vec!["mcp-server-1".to_string()],
        install_url: Some("https://opencode.ai".to_string()),
        registry_key: Some("opencode".to_string()),
        local_confirmation: AcpAgentLocalConfirmation {
            confirmed_on_this_device: true,
            confirmed_at: Some("2026-04-29T21:00:00Z".to_string()),
        },
    };

    let file_value = original.to_file_value();
    let restored = AcpAgentConfig::from_file_value(&file_value).unwrap();
    assert_eq!(original, restored);
}

#[derive(Default)]
struct TestSecureStorage {
    values: HashMap<String, String>,
}

impl TestSecureStorage {
    fn new(values: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>) -> Self {
        Self {
            values: values
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect(),
        }
    }
}

impl warpui_extras::secure_storage::SecureStorage for TestSecureStorage {
    fn write_value(
        &self,
        _key: &str,
        _value: &str,
    ) -> Result<(), warpui_extras::secure_storage::Error> {
        Ok(())
    }

    fn read_value(&self, key: &str) -> Result<String, warpui_extras::secure_storage::Error> {
        self.values
            .get(key)
            .cloned()
            .ok_or(warpui_extras::secure_storage::Error::NotFound)
    }

    fn remove_value(&self, _key: &str) -> Result<(), warpui_extras::secure_storage::Error> {
        Ok(())
    }
}

fn remote_acp_config_with_header(header: AcpAgentHttpHeader) -> AcpAgentConfig {
    AcpAgentConfig {
        id: AcpAgentId::new("remote-agent"),
        name: "Remote Agent".to_string(),
        command: String::new(),
        transport: AcpAgentTransportConfig::Http {
            url: "https://remote.example/acp".to_string(),
            headers: vec![header],
        },
        args: Vec::new(),
        env: Vec::new(),
        mcp_allowlist: Vec::new(),
        install_url: None,
        registry_key: None,
        local_confirmation: AcpAgentLocalConfirmation::default(),
    }
}

#[test]
fn to_agent_connection_resolves_secret_ref_headers() {
    App::test((), |mut app| async move {
        app.update(|ctx| {
            ctx.add_singleton_model(|_| -> warpui_extras::secure_storage::Model {
                Box::new(TestSecureStorage::new([(
                    "my-token",
                    "Bearer resolved-token",
                )]))
            });
        });

        let config = remote_acp_config_with_header(AcpAgentHttpHeader {
            name: "Authorization".to_string(),
            value: AcpAgentEnvValue::SecretRef {
                key: "my-token".to_string(),
            },
        });

        app.read(|ctx| {
            let connection = config
                .to_agent_connection(ctx)
                .expect("secret header should materialize");
            let warp_acp::AcpAgentConnection::Http { endpoint } = connection else {
                panic!("expected HTTP ACP connection");
            };
            assert_eq!(endpoint.headers.len(), 1);
            assert_eq!(endpoint.headers[0].name, "Authorization");
            assert_eq!(endpoint.headers[0].value, "Bearer resolved-token");
        });
    });
}

#[test]
fn to_agent_connection_returns_missing_secret_error() {
    App::test((), |mut app| async move {
        app.update(|ctx| {
            ctx.add_singleton_model(|_| -> warpui_extras::secure_storage::Model {
                Box::new(TestSecureStorage::default())
            });
        });

        let config = remote_acp_config_with_header(AcpAgentHttpHeader {
            name: "Authorization".to_string(),
            value: AcpAgentEnvValue::SecretRef {
                key: "missing-token".to_string(),
            },
        });

        app.read(|ctx| {
            let error = config
                .to_agent_connection(ctx)
                .expect_err("missing secret should return structured error");
            assert!(matches!(
                error,
                AcpAgentConnectionError::MissingSecret { ref key } if key == "missing-token"
            ));
            assert!(error
                .to_string()
                .contains("configure missing secret: missing-token"));
        });
    });
}

#[test]
fn acp_agent_http_header_debug_redacts_secret_values() {
    let header = AcpAgentHttpHeader {
        name: "Authorization".to_string(),
        value: AcpAgentEnvValue::Literal {
            value: "Bearer literal-token".to_string(),
        },
    };

    let debug = format!("{header:?}");
    assert!(debug.contains("Authorization"));
    assert!(!debug.contains("literal-token"));
    assert!(debug.contains("<redacted>"));
}

#[test]
fn acp_agent_env_value_debug_redacts_literal_values() {
    let value = AcpAgentEnvValue::Literal {
        value: "sk-test-token".to_string(),
    };

    let debug = format!("{value:?}");
    assert!(!debug.contains("sk-test-token"));
    assert!(debug.contains("<redacted>"));
}

#[test]
fn local_acp_agent_requires_device_confirmation_before_launch() {
    let config = AcpAgentConfig {
        id: AcpAgentId::new("local"),
        name: "Local".to_string(),
        command: "opencode".to_string(),
        transport: AcpAgentTransportConfig::Local,
        args: vec!["acp".to_string()],
        env: Vec::new(),
        mcp_allowlist: Vec::new(),
        install_url: None,
        registry_key: None,
        local_confirmation: AcpAgentLocalConfirmation::default(),
    };

    assert!(matches!(
        config.ensure_local_launch_confirmed(),
        Err(AcpAgentConnectionError::LocalConfirmationRequired { .. })
    ));
}

#[test]
fn test_configured_acp_agents_are_feature_gated() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        let config = AcpAgentConfig::from_registry_entry(&KNOWN_ACP_AGENTS[0]);
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .acp_agent_configs
                .set_value(vec![config.clone()], ctx)
                .unwrap();
        });

        let _disabled = FeatureFlag::AcpClient.override_enabled(false);
        AISettings::handle(&app).read(&app, |settings, _ctx| {
            assert!(settings.configured_acp_agents().is_empty());
            assert!(AISettings::known_acp_agent_registry().is_empty());
        });
        drop(_disabled);

        let _enabled = FeatureFlag::AcpClient.override_enabled(true);
        AISettings::handle(&app).read(&app, |settings, _ctx| {
            assert_eq!(
                settings.configured_acp_agents(),
                std::slice::from_ref(&config)
            );
            assert_eq!(
                settings.acp_agent_config(&AcpAgentId::new("opencode")),
                Some(&config)
            );
            assert!(!AISettings::known_acp_agent_registry().is_empty());
        });
    });
}

#[test]
fn test_configured_acp_agents_enable_local_agent_entrypoints() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        let _enabled = FeatureFlag::AcpClient.override_enabled(true);
        AISettings::handle(&app).read(&app, |settings, _ctx| {
            assert!(!settings.has_configured_local_acp_agents());
        });

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings.add_acp_agent_from_registry_entry("opencode", ctx);
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            assert!(settings.has_configured_local_acp_agents());
        });
    });
}

#[test]
fn test_add_and_remove_acp_agent_from_registry() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        let _enabled = FeatureFlag::AcpClient.override_enabled(true);
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings.add_acp_agent_from_registry_entry("opencode", ctx);
            settings.add_acp_agent_from_registry_entry("opencode", ctx);
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            assert_eq!(settings.configured_acp_agents().len(), 1);
            assert_eq!(
                settings.configured_acp_agents()[0].id,
                AcpAgentId::new("opencode")
            );
            assert_eq!(
                settings.configured_acp_agents()[0].args,
                &["acp", "--port", "0"]
            );
        });

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings.remove_acp_agent_config(&AcpAgentId::new("opencode"), ctx);
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            assert!(settings.configured_acp_agents().is_empty());
        });
    });
}

#[test]
fn test_add_custom_acp_agent_config_generates_unique_local_ids() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        let _enabled = FeatureFlag::AcpClient.override_enabled(true);
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings.add_custom_acp_agent_config(
                "My Local Agent",
                "custom-acp",
                vec!["--stdio".to_string()],
                ctx,
            );
            settings.add_custom_acp_agent_config("My Local Agent", "custom-acp", Vec::new(), ctx);
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            let configs = settings.configured_acp_agents();
            assert_eq!(configs.len(), 2);
            assert_eq!(configs[0].id, AcpAgentId::new("my-local-agent"));
            assert_eq!(configs[0].args, &["--stdio"]);
            assert_eq!(configs[1].id, AcpAgentId::new("my-local-agent-2"));
            assert!(configs[0].local_confirmation.confirmed_on_this_device);
        });
    });
}

#[test]
fn test_upsert_acp_agent_config_replaces_existing_agent() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        let _enabled = FeatureFlag::AcpClient.override_enabled(true);
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings.add_custom_acp_agent_config("Local Agent", "old-acp", Vec::new(), ctx);
            settings.upsert_acp_agent_config(
                AcpAgentConfig {
                    id: AcpAgentId::new("local-agent"),
                    name: "Local Agent Updated".to_string(),
                    command: "new-acp".to_string(),
                    transport: AcpAgentTransportConfig::Local,
                    args: vec!["--stdio".to_string()],
                    env: vec![AcpAgentEnvVar {
                        name: "TOKEN".to_string(),
                        value: AcpAgentEnvValue::SecretRef {
                            key: "token".to_string(),
                        },
                    }],
                    mcp_allowlist: vec!["server-uuid".to_string()],
                    install_url: Some("https://example.test".to_string()),
                    registry_key: None,
                    local_confirmation: AcpAgentLocalConfirmation {
                        confirmed_on_this_device: true,
                        confirmed_at: None,
                    },
                },
                ctx,
            );
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            let configs = settings.configured_acp_agents();
            assert_eq!(configs.len(), 1);
            assert_eq!(configs[0].name, "Local Agent Updated");
            assert_eq!(configs[0].command, "new-acp");
            assert_eq!(configs[0].args, &["--stdio"]);
            assert_eq!(configs[0].mcp_allowlist, &["server-uuid"]);
            assert!(matches!(
                configs[0].env[0].value,
                AcpAgentEnvValue::SecretRef { .. }
            ));
        });
    });
}

#[test]
fn test_toolbar_command_map_matched_agent() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        let mut map = IndexMap::new();
        map.insert("^claude".to_string(), "Claude".to_string());
        map.insert("^gemini".to_string(), "Gemini".to_string());
        map.insert("^custom-tool".to_string(), String::new());

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            report_if_error!(settings
                .cli_agent_footer_enabled_commands
                .set_value(ToolbarCommandMap::new(map), ctx));
        });

        app.read(|ctx| {
            let agent = CompiledCommandsForCodingAgentToolbar::matched_agent(ctx, "claude chat");
            assert_eq!(agent, Some(CLIAgent::Claude));

            let agent = CompiledCommandsForCodingAgentToolbar::matched_agent(ctx, "gemini ask");
            assert_eq!(agent, Some(CLIAgent::Gemini));

            let agent =
                CompiledCommandsForCodingAgentToolbar::matched_agent(ctx, "custom-tool --flag");
            assert_eq!(agent, Some(CLIAgent::Unknown));

            let agent =
                CompiledCommandsForCodingAgentToolbar::matched_agent(ctx, "unmatched-command");
            assert_eq!(agent, None);
        });
    });
}

#[test]
fn test_should_display_quota_reset_banner_with_empty_history() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // With empty history, banner should not be displayed
            assert!(!settings.should_display_quota_reset_banner());
        });
    });
}

#[test]
fn test_should_display_quota_reset_banner_with_quota_exceeded_not_dismissed() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        // Set up a history with a previous cycle that had quota exceeded and banner not dismissed
        let now = Utc::now();
        let previous_end_date = now - chrono::Duration::days(15);
        let current_end_date = now + chrono::Duration::days(15);

        let previous_cycle = CycleInfo {
            end_date: previous_end_date,
            was_quota_exceeded: true,
            banner_state: BannerState { dismissed: false },
        };

        let current_cycle = CycleInfo {
            end_date: current_end_date,
            was_quota_exceeded: false,
            banner_state: BannerState::default(),
        };

        let cycle_history = vec![previous_cycle, current_cycle];

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_request_quota_info
                .set_value(AIRequestQuotaInfo { cycle_history }, ctx)
                .unwrap();
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Banner should be displayed when the previous cycle had quota exceeded and banner not dismissed
            assert!(settings.should_display_quota_reset_banner());
        });
    });
}

#[test]
fn test_should_display_quota_reset_banner_with_quota_exceeded_dismissed() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        // Set up a history with a previous cycle that had quota exceeded but banner was dismissed
        let now = Utc::now();
        let previous_end_date = now - chrono::Duration::days(15);
        let current_end_date = now + chrono::Duration::days(15);

        let previous_cycle = CycleInfo {
            end_date: previous_end_date,
            was_quota_exceeded: true,
            banner_state: BannerState { dismissed: true },
        };

        let current_cycle = CycleInfo {
            end_date: current_end_date,
            was_quota_exceeded: false,
            banner_state: BannerState::default(),
        };

        let cycle_history = vec![previous_cycle, current_cycle];

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_request_quota_info
                .set_value(AIRequestQuotaInfo { cycle_history }, ctx)
                .unwrap();
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Banner should not be displayed when the previous cycle had quota exceeded but banner was dismissed
            assert!(!settings.should_display_quota_reset_banner());
        });
    });
}

#[test]
fn test_should_display_quota_reset_banner_with_quota_not_exceeded() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        // Set up a history with a previous cycle that did not have quota exceeded
        let now = Utc::now();
        let previous_end_date = now - chrono::Duration::days(15);
        let current_end_date = now + chrono::Duration::days(15);

        let previous_cycle = CycleInfo {
            end_date: previous_end_date,
            was_quota_exceeded: false,
            banner_state: BannerState::default(),
        };

        let current_cycle = CycleInfo {
            end_date: current_end_date,
            was_quota_exceeded: false,
            banner_state: BannerState::default(),
        };

        let cycle_history = vec![previous_cycle, current_cycle];

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_request_quota_info
                .set_value(AIRequestQuotaInfo { cycle_history }, ctx)
                .unwrap();
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Banner should not be displayed when the previous cycle did not have quota exceeded
            assert!(!settings.should_display_quota_reset_banner());
        });
    });
}

#[test]
fn test_should_display_quota_reset_banner_with_only_one_cycle() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        // Set up a history with only one cycle
        let now = Utc::now();
        let current_end_date = now + chrono::Duration::days(15);

        let current_cycle = CycleInfo {
            end_date: current_end_date,
            was_quota_exceeded: true, // Even if quota is exceeded
            banner_state: BannerState::default(),
        };

        let cycle_history = vec![current_cycle];

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_request_quota_info
                .set_value(AIRequestQuotaInfo { cycle_history }, ctx)
                .unwrap();
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Banner should not be displayed when there's only one cycle, even if quota is exceeded
            assert!(!settings.should_display_quota_reset_banner());
        });
    });
}

#[test]
fn test_update_quota_info_create_new_cycle_when_none_exists() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        let now = Utc::now();
        let next_refresh = now + chrono::Duration::days(30);

        // Create a request limit info with quota not exceeded
        let request_limit_info = create_test_request_limit_info(
            100, // limit
            50,  // used
            next_refresh,
            false, // not unlimited
            RequestLimitRefreshDuration::Monthly,
        );

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            // Ensure we start with empty history
            settings
                .ai_request_quota_info
                .set_value(
                    AIRequestQuotaInfo {
                        cycle_history: vec![],
                    },
                    ctx,
                )
                .unwrap();

            // Update quota info
            settings.update_quota_info(&request_limit_info, ctx);
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Verify a new cycle was created
            let cycle_history = &settings.ai_request_quota_info.cycle_history;
            assert_eq!(cycle_history.len(), 1);

            let cycle = &cycle_history[0];
            assert_eq!(cycle.end_date, next_refresh);
            assert!(!cycle.was_quota_exceeded);
            assert!(!cycle.banner_state.dismissed);
        });
    });
}

#[test]
fn test_update_quota_info_update_existing_cycle() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        let now = Utc::now();
        let cycle_end_date = now + chrono::Duration::days(30);

        // Set up an existing cycle
        let existing_cycle = CycleInfo {
            end_date: cycle_end_date,
            was_quota_exceeded: false,
            banner_state: BannerState::default(),
        };

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_request_quota_info
                .set_value(
                    AIRequestQuotaInfo {
                        cycle_history: vec![existing_cycle],
                    },
                    ctx,
                )
                .unwrap();
        });

        // Create a request limit info with updated usage
        let request_limit_info = create_test_request_limit_info(
            100, // limit
            75,  // used (increased)
            cycle_end_date,
            false, // not unlimited
            RequestLimitRefreshDuration::Monthly,
        );

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            // Update quota info
            settings.update_quota_info(&request_limit_info, ctx);
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Verify the cycle was updated
            let cycle_history = &settings.ai_request_quota_info.cycle_history;
            assert_eq!(cycle_history.len(), 1);

            let cycle = &cycle_history[0];
            assert_eq!(cycle.end_date, cycle_end_date);
            assert!(!cycle.was_quota_exceeded);
        });
    });
}

#[test]
fn test_update_quota_info_quota_exceeded() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        let now = Utc::now();
        let next_refresh = now + chrono::Duration::days(30);

        // Create a request limit info with quota exceeded
        let request_limit_info = create_test_request_limit_info(
            100, // limit
            100, // used (equal to limit, should be marked as exceeded)
            next_refresh,
            false, // not unlimited
            RequestLimitRefreshDuration::Monthly,
        );

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            // Update quota info
            settings.update_quota_info(&request_limit_info, ctx);
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Verify quota exceeded is set correctly
            let cycle_history = &settings.ai_request_quota_info.cycle_history;
            let cycle = &cycle_history[0];
            assert!(cycle.was_quota_exceeded);
        });

        // Test with unlimited requests (should never be exceeded)
        let unlimited_request_limit_info = create_test_request_limit_info(
            100, // limit
            200, // used (exceeds limit)
            next_refresh,
            true, // unlimited
            RequestLimitRefreshDuration::Monthly,
        );

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            // Update quota info
            settings.update_quota_info(&unlimited_request_limit_info, ctx);
        });

        AISettings::handle(&app).read(&app, |settings, _ctx| {
            // Verify quota exceeded is not set for unlimited plan
            let cycle_history = &settings.ai_request_quota_info.cycle_history;
            let cycle = &cycle_history[0];
            assert!(!cycle.was_quota_exceeded);
        });
    });
}

#[test]
fn test_mark_quota_banner_as_dismissed() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        let now = Utc::now();

        // Create test cycles: two expired cycles and one future cycle
        let expired_cycle_1 = CycleInfo {
            end_date: now - chrono::Duration::days(30), // 30 days ago
            was_quota_exceeded: true,
            banner_state: BannerState { dismissed: false },
        };

        let expired_cycle_2 = CycleInfo {
            end_date: now - chrono::Duration::days(15), // 15 days ago
            was_quota_exceeded: true,
            banner_state: BannerState { dismissed: false },
        };

        let future_cycle = CycleInfo {
            end_date: now + chrono::Duration::days(15), // 15 days in future
            was_quota_exceeded: false,
            banner_state: BannerState { dismissed: false },
        };

        let cycle_history = vec![expired_cycle_1, expired_cycle_2, future_cycle];

        // Set up initial state
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_request_quota_info
                .set_value(AIRequestQuotaInfo { cycle_history }, ctx)
                .unwrap();
        });

        // Mark expired cycles as dismissed
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings.mark_quota_banner_as_dismissed(ctx);
        });

        // Verify the results
        AISettings::handle(&app).read(&app, |settings, _ctx| {
            let cycle_history = &settings.ai_request_quota_info.cycle_history;
            assert_eq!(cycle_history.len(), 3);

            // First cycle (oldest expired) should be dismissed
            assert!(cycle_history[0].banner_state.dismissed);
            // Second cycle (more recent expired) should be dismissed
            assert!(cycle_history[1].banner_state.dismissed);
            // Future cycle should not be dismissed
            assert!(!cycle_history[2].banner_state.dismissed);
        });
    });
}
