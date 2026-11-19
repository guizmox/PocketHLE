//! Semantic launch requirements. Presentation and input preferences are deliberately absent.
use crate::LaunchContext;
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub enum Engine {
    Arm,
    CompactFramework,
}
pub fn environment(context: &LaunchContext, engine: Engine) -> BTreeMap<String, String> {
    let game = &context.game;
    let config = &context.config;
    let gizmondo = pocket_library::is_gizmondo_game(game, &context.library);
    let mut fields = BTreeMap::new();
    let native = matches!(engine, Engine::Arm);
    fields.insert(
        "Execution engine".into(),
        if native {
            "ARM WinCE"
        } else {
            "Compact Framework"
        }
        .into(),
    );
    fields.insert(
        "Device".into(),
        if gizmondo { "Gizmondo" } else { "Pocket PC" }.into(),
    );
    // Keep the requested panel policy stable across launcher -> child variants.
    // Runtime display changes are part of the captured kernel, not new settings.
    fields.insert(
        "Guest display policy".into(),
        if gizmondo {
            format!("{:?}", game.settings.screen.gizmondo_size())
        } else {
            format!("{:?}", game.settings.screen)
        },
    );
    fields.insert("GPU rendering".into(), config.gpu_renderer.to_string());
    fields.insert(
        "Internal resolution policy".into(),
        config.gles_internal_resolution.to_string(),
    );
    if native {
        fields.insert(
            "Executable heap".into(),
            (gizmondo || game.settings.effective_executable_heap(config)).to_string(),
        );
        let (preset, platform) = config.device_identity_hex(gizmondo);
        let preset = if !gizmondo && !game.settings.pocketpc_preset_id.trim().is_empty() {
            game.settings.pocketpc_preset_id.trim()
        } else {
            preset
        };
        fields.insert("Device preset".into(), preset.to_ascii_lowercase());
        fields.insert("Device platform".into(), platform.to_ascii_lowercase());
        if !gizmondo {
            fields.insert(
                "Processor".into(),
                config.pocketpc_processor.to_ascii_lowercase(),
            );
            fields.insert(
                "Graphics profile".into(),
                game.settings.gpu_compatibility_profile.label().into(),
            );
            fields.insert(
                "EGL formats".into(),
                format!("{:?}", game.settings.egl_surface_format),
            );
            fields.insert(
                "GAPI columns".into(),
                game.settings.pocketpc_gapi_columns.to_string(),
            );
            for (key, value) in [
                ("Owner name", &game.settings.pocketpc_owner_name),
                ("Device name", &game.settings.pocketpc_device_name),
                ("Device ID", &game.settings.pocketpc_device_id),
                ("OEM override", &game.settings.pocketpc_oem_info),
                ("Platform override", &game.settings.pocketpc_platform_type),
                ("Random seed policy", &game.settings.pocketpc_random_seed),
            ] {
                fields.insert(key.into(), value.clone());
            }
        }
    }
    fields
}

pub fn validate(
    saved: &BTreeMap<String, String>,
    current: &BTreeMap<String, String>,
) -> Result<(), String> {
    let mut keys: std::collections::BTreeSet<_> = saved.keys().collect();
    keys.extend(current.keys());
    let differences: Vec<_> = keys
        .into_iter()
        .filter_map(|key| {
            let before = saved.get(key);
            let now = current.get(key);
            (before != now).then(|| {
                format!(
                    "{key}: saved {}, current {}",
                    before.map_or("<missing>", String::as_str),
                    now.map_or("<missing>", String::as_str)
                )
            })
        })
        .collect();
    if differences.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Snapshot emulation settings are incompatible:\n{}",
            differences.join("\n")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mismatches_are_complete_and_deterministic() {
        let saved = BTreeMap::from([
            ("Processor".into(), "pxa270".into()),
            ("Guest display".into(), "320x240".into()),
        ]);
        assert!(validate(&saved, &saved).is_ok());
        let current = BTreeMap::from([
            ("Processor".into(), "arm1136".into()),
            ("Guest display".into(), "640x480".into()),
        ]);
        let text = validate(&saved, &current).unwrap_err();
        assert!(text.contains("saved 320x240, current 640x480"));
        assert!(text.contains("saved pxa270, current arm1136"));
        assert!(text.find("Guest display").unwrap() < text.find("Processor").unwrap());
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    fn fixture() -> LaunchContext {
        let game=pocket_library::GameEntry{
            ..serde_json::from_value(serde_json::json!({"id":"fixture","display_name":"Fixture","executable":"extracted/game.exe","source_cab":"fixture.cab","platform":"pocket_pc"})).unwrap()
        };
        LaunchContext {
            library: "/library".into(),
            card: "/library/games/fixture/extracted".into(),
            game,
            config: Default::default(),
        }
    }
    #[test]
    fn presentation_input_and_provider_preferences_do_not_invalidate_archives() {
        let context = fixture();
        let mut changed = context.clone();
        changed.config.desktop_vsync = !changed.config.desktop_vsync;
        changed.config.gps_enabled = !changed.config.gps_enabled;
        changed.config.camera_enabled = !changed.config.camera_enabled;
        changed.game.settings.pocketpc_stylus_grid.columns = 7;
        changed.game.settings.pocketpc_stylus_grid.rows = 8;
        let mut settings = serde_json::to_value(&changed.game.settings).unwrap();
        settings["rotation"] = "cw90".into();
        changed.game.settings = serde_json::from_value(settings).unwrap();
        for engine in [Engine::Arm, Engine::CompactFramework] {
            assert_eq!(environment(&context, engine), environment(&changed, engine));
        }
    }
    #[test]
    fn native_requirements_are_not_imposed_on_the_managed_interpreter() {
        let context = fixture();
        let mut changed = context.clone();
        changed.config.pocketpc_processor = "arm1136".into();
        changed.game.settings.gpu_compatibility_profile =
            pocket_library::gpu_profile::GpuCompatibilityProfile::NvidiaGoforce5500;
        assert_ne!(
            environment(&context, Engine::Arm),
            environment(&changed, Engine::Arm)
        );
        assert_eq!(
            environment(&context, Engine::CompactFramework),
            environment(&changed, Engine::CompactFramework)
        );
        changed.config.gles_internal_resolution = 2;
        assert_ne!(
            environment(&context, Engine::CompactFramework),
            environment(&changed, Engine::CompactFramework)
        );
    }
}
