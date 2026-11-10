//! JNI bridge between the PocketHLE core/library and the Android
//! frontend. The Java side talks to this crate exclusively through
//! UTF-8 JSON strings — that keeps the FFI surface small and removes
//! any need for shared Kotlin/Rust data classes.
//!
//! Exposed methods (all on `com.pockethle.app.NativeBridge`):
//!
//! * `banner()` — sanity string showing the loaded version.
//! * `listGames(libraryRoot)` — JSON array of [`pocket_library::GameEntry`].
//! * `prepareImport` / cabinet-platform choice / `commitPreparedImport` — transactional import.
//! * `removeGame(libraryRoot, id)` — `"ok"` or `"err: ..."`.
//! * `readConfig(libraryRoot)` — JSON of [`pocket_library::LauncherConfig`].
//! * `writeConfig(libraryRoot, json)` — overwrite config with a JSON blob.
//! * `readGameSettings(libraryRoot, id)` — JSON of per-game settings.
//! * `writeGameSettings(libraryRoot, id, json)` — persist per-game settings.
//! * `nativeStartGame` / `nativePollFrame` / `nativeSendInput` /
//!   `nativeRequestStop` / `nativeFinishGame` — session-based
//!   replacement that streams framebuffers to the UI as the guest
//!   draws them, forwards touches and D-pad presses to the kernel,
//!   and lets the user quit cleanly via Back. See [`runner`] for
//!   the implementation rationale.

mod bluetooth;
mod camera;
mod colors_trace;
mod gps;
mod graphics;
mod internet;
mod managed_game;
mod power;
mod runner;
mod unimplemented_log;
mod vibration;

use anyhow::Context;
use std::path::Path;
use std::path::PathBuf;

use jni::objects::{JClass, JString};
use jni::sys::{jbyteArray, jint, jlong, jshortArray, jstring};
use jni::JNIEnv;

use pocket_core::kernel::InputEvent;
use pocket_library::{GameSettings, Library};

use crate::runner::{InputCommand, Session};

// A running or still-joining session can write saves. Keep backups/removal exclusive
// with session creation, and retain the count until the worker has joined.
static LIBRARY_SESSIONS: std::sync::Mutex<usize> = std::sync::Mutex::new(0);

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_libraryBackup<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
    archive: JString<'local>,
    mode: jint,
) -> jstring {
    let result = (|| -> anyhow::Result<()> {
        let sessions = LIBRARY_SESSIONS
            .lock()
            .map_err(|_| anyhow::anyhow!("Library lock unavailable"))?;
        anyhow::ensure!(
            *sessions == 0,
            "Stop the running game before backup/restore"
        );
        let root = jstring_to_path(&mut env, library_root).context("Missing library root")?;
        let archive = jstring_to_path(&mut env, archive).context("Missing ZIP path")?;
        use pocket_library::backup::{self, RestoreMode};
        match mode {
            -1 => backup::save(&root, &archive),
            0 => backup::restore(&root, &archive, RestoreMode::MergeNoOverwrite),
            1 => backup::restore(&root, &archive, RestoreMode::MergeOverwrite),
            2 => backup::restore(&root, &archive, RestoreMode::Replace),
            _ => anyhow::bail!("Unknown restore mode"),
        }
    })();
    new_jstring(
        &env,
        match result {
            Ok(()) => serde_json::json!({"ok": true}).to_string(),
            Err(error) => error_json(&format!("{error:#}")),
        },
    )
}

fn about_banner() -> String {
    format!("PocketHLE v{} (Android)\nFork: https://github.com/guizmox/PocketHLE\nOriginal project — j92580498-max\nhttps://github.com/j92580498-max/PocketHLE\n\nGTShell support — Katamaran21\nhttps://github.com/Katamaran21/PocketHLE-GTShell\n\nThanks\nNekomo\nOLego\nTimofeyLednev\nBenoît (FX-man)\nJérôme (SoldatGG)\nTim (Firefly)\nMarat\ntorrenia", env!("CARGO_PKG_VERSION"))
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_banner<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    init_logger();
    let banner = about_banner();
    new_jstring(&env, banner)
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_MainActivity_banner<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    init_logger();
    new_jstring(&env, about_banner())
}

/// Explicit offline save action from Colors' startup dialog, before game launch.
#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_unlockColorsTurfWars<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
    id: JString<'local>,
) -> jstring {
    let result = (|| -> anyhow::Result<String> {
        let root = jstring_to_path(&mut env, library_root).context("missing root")?;
        let id = jstring_to_string(&mut env, id).context("missing ID")?;
        let library = Library::open(&root)?;
        let game = library
            .games()
            .iter()
            .find(|game| game.id == id)
            .context("unknown game")?;
        pocket_library::colors_save::unlock_turf_wars(game, &root)
    })();
    new_jstring(
        &env,
        match result {
            Ok(message) => serde_json::json!({"ok": true, "message": message}).to_string(),
            Err(error) => error_json(&format!("Could not unlock Colors menus: {error:#}")),
        },
    )
}

/// Available languages for the selected Colors installation, not another variant.
#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_colorsLanguages<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
    id: JString<'local>,
) -> jstring {
    let result = (|| -> anyhow::Result<String> {
        let root = jstring_to_path(&mut env, library_root)
            .ok_or_else(|| anyhow::anyhow!("missing root"))?;
        let id = jstring_to_string(&mut env, id).ok_or_else(|| anyhow::anyhow!("missing ID"))?;
        let lib = Library::open(&root)?;
        let game = lib
            .games()
            .iter()
            .find(|game| game.id == id)
            .ok_or_else(|| anyhow::anyhow!("unknown game"))?;
        Ok(serde_json::to_string(&pocket_library::colors_languages(
            game, &root,
        ))?)
    })();
    match result {
        Ok(body) => new_jstring(&env, body),
        Err(error) => new_jstring(&env, error_json(&format!("colorsLanguages: {error:#}"))),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_listGames<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
) -> jstring {
    init_logger();
    let root = match jstring_to_path(&mut env, library_root) {
        Some(p) => p,
        None => return new_jstring(&env, error_json("missing library root")),
    };
    let result = (|| -> anyhow::Result<String> {
        let lib = Library::open(&root)?;
        let games = lib
            .games()
            .iter()
            .map(|game| {
                let mut value = serde_json::to_value(game)?;
                value["is_gizmondo"] = pocket_library::is_gizmondo_game(game, &root).into();
                value["title_id"] =
                    serde_json::to_value(pocket_library::game_gizmondo_title_id(game, &root))?;
                let (width, height) = game.resolved_screen_size(&root);
                value["screen_width"] = width.into();
                value["screen_height"] = height.into();
                value["custom_settings"] = game.settings.has_custom_settings(lib.config()).into();
                value["graphics_api"] =
                    serde_json::to_value(pocket_library::analysis::detect_graphics_api_cached(
                        &game.executable_path(&root),
                    ))?;
                Ok(value)
            })
            .collect::<Result<Vec<serde_json::Value>, serde_json::Error>>()?;
        Ok(serde_json::to_string(&games)?)
    })();
    match result {
        Ok(j) => new_jstring(&env, j),
        Err(e) => new_jstring(&env, error_json(&format!("listGames: {e:#}"))),
    }
}

// Prepared imports own their temporary trees until the Activity confirms or
// cancels. Tokens are opaque IDs, never guest pointers or arbitrary host paths.
static PREPARED_IMPORTS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<u64, (PathBuf, pocket_library::PreparedImport)>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
static NEXT_IMPORT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn take_prepared_import(root: &Path, token: u64) -> anyhow::Result<pocket_library::PreparedImport> {
    let mut imports = PREPARED_IMPORTS.lock().unwrap_or_else(|e| e.into_inner());
    anyhow::ensure!(
        imports.get(&token).is_some_and(|(owner, _)| owner == root),
        "unknown import token or wrong library"
    );
    Ok(imports.remove(&token).unwrap().1)
}

fn prepared_import_json(token: u64, pending: &pocket_library::PreparedImport) -> String {
    let choices: Vec<_> = pending
        .platform_choices()
        .into_iter()
        .map(|(id, display_name)| serde_json::json!({"id":id,"display_name":display_name}))
        .collect();
    serde_json::json!({"ok":true,"token":token.to_string(),"count":pending.entries().len(),"updates":pending.updates(),"platform_choices":choices,"cabinet_choices":pending.cabinet_choices()}).to_string()
}
#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_choosePreparedImportCabinet<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
    token: jlong,
    index: jni::sys::jint,
) -> jstring {
    let result = (|| -> anyhow::Result<String> {
        let root = jstring_to_path(&mut env, library_root).context("missing library root")?;
        anyhow::ensure!(index >= 0, "invalid cabinet selection");
        let mut pending = take_prepared_import(&root, token as u64)?;
        pending.choose_cabinet(index as usize)?;
        let json = prepared_import_json(token as u64, &pending);
        PREPARED_IMPORTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(token as u64, (root, pending));
        Ok(json)
    })();
    new_jstring(
        &env,
        result.unwrap_or_else(|e| error_json(&format!("choosePreparedImportCabinet: {e:#}"))),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_choosePreparedImportPlatform<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
    token: jlong,
    game_id: JString<'local>,
    gizmondo: jni::sys::jboolean,
) -> jstring {
    let result = (|| -> anyhow::Result<String> {
        let root = jstring_to_path(&mut env, library_root).context("missing library root")?;
        let id: String = env.get_string(&game_id)?.into();
        let mut imports = PREPARED_IMPORTS.lock().unwrap_or_else(|e| e.into_inner());
        let (owner, pending) = imports
            .get_mut(&(token as u64))
            .context("import no longer pending")?;
        anyhow::ensure!(*owner == root, "import belongs to another library");
        let platform = if gizmondo != 0 {
            pocket_library::GamePlatform::Gizmondo
        } else {
            pocket_library::GamePlatform::PocketPc
        };
        anyhow::ensure!(
            pending.choose_platform(&id, platform),
            "game does not need a platform choice"
        );
        Ok(prepared_import_json(token as u64, pending))
    })();
    new_jstring(
        &env,
        result.unwrap_or_else(|e| error_json(&format!("choosePreparedImportPlatform: {e:#}"))),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_prepareImport<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
    source: JString<'local>,
) -> jstring {
    let result = (|| -> anyhow::Result<String> {
        let root = jstring_to_path(&mut env, library_root).context("missing library root")?;
        let path = jstring_to_path(&mut env, source).context("missing import source")?;
        let pending = Library::open(&root)?.prepare_import(path)?;
        let token = NEXT_IMPORT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let json = prepared_import_json(token, &pending);
        let mut imports = PREPARED_IMPORTS.lock().unwrap_or_else(|e| e.into_inner());
        anyhow::ensure!(imports.len() < 16, "too many pending imports");
        imports.insert(token, (root, pending));
        Ok(json.to_string())
    })();
    new_jstring(
        &env,
        result.unwrap_or_else(|e| error_json(&format!("prepareImport: {e:#}"))),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_commitPreparedImport<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
    token: jlong,
    allow_updates: jni::sys::jboolean,
) -> jstring {
    let result = (|| -> anyhow::Result<String> {
        let root = jstring_to_path(&mut env, library_root).context("missing library root")?;
        let pending = take_prepared_import(&root, token as u64)?;
        let mut library = Library::open(&root)?;
        let before = pocket_library::import_setup::Presence::read(library.games(), &root);
        let entries = library.apply_prepared_import(pending, allow_updates != 0)?;
        let setup_requests = library.config().first_import_setup.requests(
            before,
            pocket_library::import_setup::Presence::read(&entries, &root),
        );
        let mut values = entries
            .iter()
            .map(|entry| -> anyhow::Result<serde_json::Value> {
                let mut value = serde_json::to_value(entry)?;
                value["is_gizmondo"] = pocket_library::is_gizmondo_game(entry, &root).into();
                if allow_updates == 0
                    && !pocket_library::is_gizmondo_game(entry, &root)
                    && !library.is_demo_card(&entry.id)
                {
                    value["import_settings_message"] =
                        pocket_library::gpu_profile::ImportAdvice::analyze(
                            &entry.executable_path(&root),
                        )
                        .message()
                        .into();
                }
                Ok(value)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        if let Some(first) = values.first_mut() {
            first["first_import_setup"] = serde_json::to_value(setup_requests)?;
        }
        if entries.len() == 1 && !entries[0].source_cab.starts_with("gzga-collection:") {
            Ok(serde_json::to_string(&values[0])?)
        } else {
            Ok(serde_json::to_string(&values)?)
        }
    })();
    new_jstring(
        &env,
        result.unwrap_or_else(|e| error_json(&format!("commitPreparedImport: {e:#}"))),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_cancelPreparedImport<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
    token: jlong,
) {
    if let Some(root) = jstring_to_path(&mut env, library_root) {
        let _ = take_prepared_import(&root, token as u64);
    }
}

// A prepared addition owns its staged files until explicit Yes or Cancel.
static PREPARED_DATA: std::sync::LazyLock<
    std::sync::Mutex<
        std::collections::HashMap<u64, (PathBuf, pocket_library::add_data::PreparedData)>,
    >,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn take_prepared_data(
    root: &Path,
    token: u64,
) -> anyhow::Result<pocket_library::add_data::PreparedData> {
    let mut pending = PREPARED_DATA.lock().unwrap_or_else(|e| e.into_inner());
    anyhow::ensure!(
        pending.get(&token).is_some_and(|(owner, _)| owner == root),
        "Unknown data token or wrong library"
    );
    Ok(pending.remove(&token).unwrap().1)
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_prepareGameData<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
    id: JString<'local>,
    source: JString<'local>,
    name: JString<'local>,
    extract: jni::sys::jboolean,
) -> jstring {
    let result = (|| -> anyhow::Result<String> {
        let root = jstring_to_path(&mut env, library_root).context("Missing library root")?;
        let id = jstring_to_string(&mut env, id).context("Missing game id")?;
        let source = jstring_to_path(&mut env, source).context("Missing source")?;
        let name = jstring_to_string(&mut env, name).context("Missing filename")?;
        let sessions = LIBRARY_SESSIONS
            .lock()
            .map_err(|_| anyhow::anyhow!("Library lock unavailable"))?;
        anyhow::ensure!(*sessions == 0, "Stop the running game before adding data");
        let pending = pocket_library::add_data::prepare(
            &Library::open(&root)?,
            &id,
            &source,
            &name,
            extract != 0,
        )?;
        let token = NEXT_IMPORT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let json = serde_json::json!({"ok": true, "token": token.to_string(), "conflicts": pending.conflicts(), "executables": pending.executables()}).to_string();
        PREPARED_DATA
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(token, (root, pending));
        Ok(json)
    })();
    new_jstring(
        &env,
        result.unwrap_or_else(|e| error_json(&format!("{e:#}"))),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_commitGameData<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
    token: jlong,
    overwrite: jni::sys::jboolean,
    executable: JString<'local>,
) -> jstring {
    let result = (|| -> anyhow::Result<String> {
        let root = jstring_to_path(&mut env, library_root).context("Missing library root")?;
        let mut pending = take_prepared_data(&root, token as u64)?;
        let executable =
            jstring_to_string(&mut env, executable).context("Missing executable choice")?;
        pending.choose_executable((!executable.is_empty()).then_some(executable.as_str()))?;
        let sessions = LIBRARY_SESSIONS
            .lock()
            .map_err(|_| anyhow::anyhow!("Library lock unavailable"))?;
        anyhow::ensure!(*sessions == 0, "Stop the running game before adding data");
        let destination = pending.destination().display().to_string();
        pending.apply(&mut Library::open(&root)?, overwrite != 0)?;
        Ok(serde_json::json!({"ok": true, "destination": destination}).to_string())
    })();
    new_jstring(
        &env,
        result.unwrap_or_else(|e| error_json(&format!("{e:#}"))),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_cancelGameData<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
    token: jlong,
) {
    if let Some(root) = jstring_to_path(&mut env, library_root) {
        let _ = take_prepared_data(&root, token as u64);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_removeGame<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
    id: JString<'local>,
) -> jstring {
    init_logger();
    let root = match jstring_to_path(&mut env, library_root) {
        Some(p) => p,
        None => return new_jstring(&env, error_json("missing library root")),
    };
    let id = match jstring_to_string(&mut env, id) {
        Some(s) => s,
        None => return new_jstring(&env, error_json("missing game id")),
    };
    let result = (|| -> anyhow::Result<()> {
        let sessions = LIBRARY_SESSIONS
            .lock()
            .map_err(|_| anyhow::anyhow!("Library lock unavailable"))?;
        anyhow::ensure!(
            *sessions == 0,
            "Stop the running game before removing a library entry"
        );

        let mut lib = Library::open(&root)?;
        lib.remove(&id)?;
        Ok(())
    })();
    match result {
        Ok(()) => new_jstring(&env, "{\"ok\":true}"),
        Err(e) => new_jstring(&env, error_json(&format!("removeGame: {e:#}"))),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_readConfig<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
) -> jstring {
    init_logger();
    let root = match jstring_to_path(&mut env, library_root) {
        Some(p) => p,
        None => return new_jstring(&env, error_json("missing library root")),
    };
    let result = (|| -> anyhow::Result<String> {
        let lib = Library::open(&root)?;
        let mut config = lib.config().clone();
        for (file, platform) in [
            ("registry-pocketpc.json", &mut config.device_platform_id),
            ("registry-gizmondo.json", &mut config.gizmondo_platform_id),
        ] {
            if platform.is_empty() {
                let mut registry = pocket_core::kernel::registry::Registry::default();
                registry.configure_persistence(&root.join(file))?;
                let id = pocket_core::kernel::device_identity::DeviceIdentity::load_or_create(
                    &mut registry,
                )
                .map_err(anyhow::Error::msg)?;
                *platform = pocket_core::kernel::device_identity::DeviceIdentity::hex(&id.platform);
            }
        }
        Ok(serde_json::to_string(&config)?)
    })();
    match result {
        Ok(j) => new_jstring(&env, j),
        Err(e) => new_jstring(&env, error_json(&format!("readConfig: {e:#}"))),
    }
}

/// Finish first-import setup by merging only the selected fields into current settings.
#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_finishImportSetup<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
    choices_json: JString<'local>,
) -> jstring {
    let result = (|| -> anyhow::Result<()> {
        let root = jstring_to_path(&mut env, library_root).context("missing library root")?;
        let body = jstring_to_string(&mut env, choices_json).context("missing setup choices")?;
        let choices: pocket_library::import_setup::Choices = serde_json::from_str(&body)?;
        pocket_library::import_setup::save(
            &root,
            pocket_library::LauncherProfile::Standard,
            &choices,
        )?;
        Ok(())
    })();
    new_jstring(
        &env,
        result
            .map(|_| "{\"ok\":true}".to_owned())
            .unwrap_or_else(|error| error_json(&format!("finishImportSetup: {error:#}"))),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_writeConfig<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
    config_json: JString<'local>,
) -> jstring {
    init_logger();
    let root = match jstring_to_path(&mut env, library_root) {
        Some(p) => p,
        None => return new_jstring(&env, error_json("missing library root")),
    };
    let body = match jstring_to_string(&mut env, config_json) {
        Some(s) => s,
        None => return new_jstring(&env, error_json("missing config json")),
    };
    let result = (|| -> anyhow::Result<()> {
        let mut lib = Library::open(&root)?;
        let cfg: pocket_library::LauncherConfig = serde_json::from_str(&body)?;
        for (preset, platform) in [
            (&cfg.device_preset_id, &cfg.device_platform_id),
            (&cfg.gizmondo_preset_id, &cfg.gizmondo_platform_id),
        ] {
            pocket_core::kernel::device_identity::DeviceIdentity::from_hex(
                preset,
                if platform.is_empty() { "00" } else { platform },
            )
            .map_err(anyhow::Error::msg)?;
        }
        *lib.config_mut() = cfg;
        lib.save()?;
        Ok(())
    })();
    match result {
        Ok(()) => new_jstring(&env, "{\"ok\":true}"),
        Err(e) => new_jstring(&env, error_json(&format!("writeConfig: {e:#}"))),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_readGameSettings<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
    id: JString<'local>,
) -> jstring {
    init_logger();
    let root = match jstring_to_path(&mut env, library_root) {
        Some(p) => p,
        None => return new_jstring(&env, error_json("missing library root")),
    };
    let id = match jstring_to_string(&mut env, id) {
        Some(s) => s,
        None => return new_jstring(&env, error_json("missing game id")),
    };
    let result = (|| -> anyhow::Result<String> {
        let lib = Library::open(&root)?;
        let entry = lib
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("unknown game"))?;
        Ok(serde_json::to_string(&entry.settings)?)
    })();
    match result {
        Ok(j) => new_jstring(&env, j),
        Err(e) => new_jstring(&env, error_json(&format!("readGameSettings: {e:#}"))),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_writeGameSettings<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
    id: JString<'local>,
    settings_json: JString<'local>,
) -> jstring {
    init_logger();
    let root = match jstring_to_path(&mut env, library_root) {
        Some(p) => p,
        None => return new_jstring(&env, error_json("missing library root")),
    };
    let id = match jstring_to_string(&mut env, id) {
        Some(s) => s,
        None => return new_jstring(&env, error_json("missing game id")),
    };
    let body = match jstring_to_string(&mut env, settings_json) {
        Some(s) => s,
        None => return new_jstring(&env, error_json("missing settings json")),
    };
    let result = (|| -> anyhow::Result<()> {
        let mut lib = Library::open(&root)?;
        let new_settings: GameSettings = serde_json::from_str(&body)?;
        lib.update_settings(&id, new_settings)?;
        Ok(())
    })();
    match result {
        Ok(()) => new_jstring(&env, "{\"ok\":true}"),
        Err(e) => new_jstring(&env, error_json(&format!("writeGameSettings: {e:#}"))),
    }
}

fn jstring_to_string<'a>(env: &mut JNIEnv<'a>, value: JString<'a>) -> Option<String> {
    if value.is_null() {
        return None;
    }
    match env.get_string(&value) {
        Ok(s) => Some(s.into()),
        Err(e) => {
            log::error!("could not read jstring: {e}");
            None
        }
    }
}

fn jstring_to_path<'a>(env: &mut JNIEnv<'a>, value: JString<'a>) -> Option<PathBuf> {
    jstring_to_string(env, value).map(PathBuf::from)
}

fn new_jstring(env: &JNIEnv<'_>, body: impl AsRef<str>) -> jstring {
    match env.new_string(body.as_ref()) {
        Ok(s) => s.into_raw(),
        Err(e) => {
            log::error!("could not allocate JString: {e}");
            std::ptr::null_mut()
        }
    }
}

fn error_json(msg: &str) -> String {
    let escaped = msg
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("{{\"ok\":false,\"error\":\"{escaped}\"}}")
}

fn init_logger() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Trace)
                .with_tag("PocketHLE"),
        );
    });
}

// ---------------------------------------------------------------------------
// Session-based emulator API
//
// A worker streams images and accepts input while the guest is running.
//
//  * `nativeStartGame` returns an opaque `jlong` handle (a pointer to a
//    boxed [`Session`]).
//  * `nativePollFrame` returns the latest framebuffer that the worker has
//    produced since the last poll, as a flat `byte[]` with an 8-byte
//    little-endian header `(width: u32, height: u32)` followed by RGBA
//    pixels. Returning `null` means "no new frame", returning a zero-length
//    array means "session already ended".
//  * `nativeSendInput` pushes a virtual-key or stylus event into the
//    kernel's pending-input queue.
//  * `nativeRequestStop` asks the worker to stop at the next slice
//    boundary.
//  * `nativeFinishGame` joins the worker and frees the session, returning
//    a textual summary that the activity shows in its status panel.
//
// The pointer is opaque to Kotlin and is consumed exactly once by
// `nativeFinishGame`. After that call the handle is invalid and Kotlin must
// not pass it back into any of the methods above.

/// `kind` ordinals exchanged with Kotlin. See `NativeBridge.kt`.
const INPUT_KIND_STYLUS_AIM: jint = 6;
const INPUT_KIND_KEY_DOWN: jint = 0;
const INPUT_KIND_KEY_UP: jint = 1;
const INPUT_KIND_POINTER_DOWN: jint = 2;
const INPUT_KIND_POINTER_UP: jint = 3;
const INPUT_KIND_POINTER_MOVE: jint = 4;
const INPUT_KIND_KEYBOARD_ROTATION: jint = 5;

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativeStartGame<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
    id: JString<'local>,
    presentation: jlong,
) -> jlong {
    init_logger();
    if let Err(e) = bluetooth::install(&mut env, &_class) {
        log::error!("Cannot initialize Android Bluetooth bridge: {e}");
        return 0;
    }
    if let Err(e) = internet::install(&mut env, &_class) {
        log::error!("Cannot initialize Android HTTP bridge: {e}");
        return 0;
    }
    if let Err(e) = vibration::install(&mut env, &_class) {
        log::error!("Cannot initialize Android vibration bridge: {e}");
        return 0;
    }
    if let Err(e) = power::install(&mut env, &_class) {
        log::error!("Cannot initialize Android battery bridge: {e}");
        return 0;
    }
    if let Err(e) = gps::install(&mut env, &_class) {
        log::error!("Cannot initialize Android GPS bridge: {e}");
        return 0;
    }
    if let Err(e) = camera::install(&mut env, &_class) {
        log::error!("Cannot initialize Android camera bridge: {e}");
        return 0;
    }
    let root = match jstring_to_path(&mut env, library_root) {
        Some(p) => p,
        None => {
            log::error!("nativeStartGame: missing library root");
            return 0;
        }
    };
    let id = match jstring_to_string(&mut env, id) {
        Some(s) => s,
        None => {
            log::error!("nativeStartGame: missing game id");
            return 0;
        }
    };
    let mut sessions = LIBRARY_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    match runner::start_with_presentation(root, id, graphics::get(presentation)) {
        Ok(session) => {
            *sessions += 1;
            Box::into_raw(Box::new(session)) as jlong
        }
        Err(e) => {
            log::error!("nativeStartGame: {e:#}");
            0
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativePublishedImageIps(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jni::sys::jfloat {
    session_from_handle(handle)
        .and_then(|session| session.published_image_ips())
        .unwrap_or(-1.0)
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativeSystemMedia(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    let Some(status) = session_from_handle(handle).and_then(Session::system_media) else {
        return std::ptr::null_mut();
    };
    let progress = (status.duration.is_finite() && status.duration > 0.0)
        .then(|| status.position() / status.duration);
    new_jstring(
        &env,
        serde_json::json!({
            "paused": status.paused,
            "progress": progress,
        })
        .to_string(),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativeSystemMediaCommand(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
    command: jint,
) {
    use pocket_core::winceapi::SystemMediaCommand;
    let command = match command {
        0 => SystemMediaCommand::TogglePause,
        1 => SystemMediaCommand::Stop,
        2 => SystemMediaCommand::Rewind,
        3 => SystemMediaCommand::Forward,
        _ => return,
    };
    if let Some(session) = session_from_handle(handle) {
        session.send_input(InputCommand::SystemMedia(command));
    }
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativePollFrame<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jbyteArray {
    let Some(session) = session_from_handle(handle) else {
        return std::ptr::null_mut();
    };
    let Some(frame) = session.poll_frame() else {
        // No new frame this tick — return null to let Kotlin keep
        // showing the previous content of the SurfaceView.
        return std::ptr::null_mut();
    };
    if frame.shared.is_some() {
        let Some(bridge) = session.presentation() else {
            return std::ptr::null_mut();
        };
        let words = [
            u32::MAX - 1,
            frame.width,
            frame.height,
            frame.texture_width,
            frame.texture_height,
        ];
        let mut header: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let token = bridge.publish(frame);
        header.extend_from_slice(&token.to_le_bytes());
        return env
            .byte_array_from_slice(&header)
            .map(|a| a.into_raw())
            .unwrap_or(std::ptr::null_mut());
    }
    let started = session.diagnostic_enabled().then(std::time::Instant::now);
    let result = (|| {
        // Native (1x) keeps the original eight-byte wire header. High resolution
        // has a negative marker, logical dimensions and physical texture dimensions.
        let words = if frame.rgb565 {
            vec![u32::MAX - 2, frame.width, frame.height, frame.backlight]
        } else if (frame.width, frame.height) == (frame.texture_width, frame.texture_height) {
            vec![frame.width, frame.height]
        } else {
            vec![
                u32::MAX,
                frame.width,
                frame.height,
                frame.texture_width,
                frame.texture_height,
            ]
        };
        let header: Vec<i8> = words
            .iter()
            .flat_map(|word| word.to_le_bytes().map(|byte| byte as i8))
            .collect();
        let total = header.len().saturating_add(frame.rgba.len());
        let Ok(total) = i32::try_from(total) else {
            return std::ptr::null_mut();
        };
        let array = match env.new_byte_array(total) {
            Ok(array) => array,
            Err(e) => {
                log::error!("could not allocate jbyteArray: {e}");
                return std::ptr::null_mut();
            }
        };
        let rgba = unsafe {
            std::slice::from_raw_parts(frame.rgba.as_ptr() as *const i8, frame.rgba.len())
        };
        if env.set_byte_array_region(&array, 0, &header).is_err()
            || env
                .set_byte_array_region(&array, header.len() as i32, rgba)
                .is_err()
        {
            log::error!("could not fill jbyteArray");
            return std::ptr::null_mut();
        }
        array.into_raw()
    })();
    if let Some(start) = started {
        session.record_jni(start.elapsed());
    }
    result
}

/// `nativeAudioFormat` — the guest PCM format packed as
/// `(sample_rate << 16) | channels`, or `0` while the game has not
/// opened its `waveOut` device yet. Kotlin needs this to size its
/// `AudioTrack` before it starts pulling samples.
#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativeAudioFormat<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jlong {
    let Some(session) = session_from_handle(handle) else {
        return 0;
    };
    match session.audio_format() {
        Some((rate, channels)) => ((rate as jlong) << 16) | channels as jlong,
        None => 0,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativePollAudio<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    max_samples: jint,
) -> jshortArray {
    let Some(session) = session_from_handle(handle) else {
        return std::ptr::null_mut();
    };
    let count = (max_samples as usize).clamp(1, 16384);
    let mut samples = vec![0i16; count];
    let n = session.poll_audio(&mut samples);
    if n == 0 {
        return std::ptr::null_mut();
    }
    let Ok(array) = env.new_short_array(n as i32) else {
        return std::ptr::null_mut();
    };
    if env
        .set_short_array_region(&array, 0, &samples[..n])
        .is_err()
    {
        return std::ptr::null_mut();
    }
    array.into_raw()
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativeIsRunning<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jint {
    match session_from_handle(handle) {
        Some(session) if session.is_running() => 1,
        _ => 0,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativePollGpsNotice<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jstring {
    let Some(position) = session_from_handle(handle).and_then(|s| s.poll_gps_notice()) else {
        return std::ptr::null_mut();
    };
    let message = format!(
        "Fake GPS position refreshed!\nLatitude: {:.6}\nLongitude: {:.6}",
        position.0, position.1
    );
    env.new_string(message)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativeSendInput<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    kind: jint,
    a: jint,
    b: jint,
) -> jint {
    let Some(session) = session_from_handle(handle) else {
        return 0;
    };
    let event = match kind {
        7 => InputEvent::TouchSteeringPosition {
            x: a.clamp(-1000, 1000) as i16,
            y: b.clamp(-1000, 1000) as i16,
        },
        INPUT_KIND_STYLUS_AIM => InputEvent::StylusAim {
            x: a.clamp(-1200, 1200) as i16,
            y: b.clamp(-1200, 1200) as i16,
        },
        INPUT_KIND_KEYBOARD_ROTATION => InputEvent::KeyboardRotation {
            turns: (a as u8) % 4,
        },
        INPUT_KIND_KEY_DOWN => InputEvent::KeyDown { vk: a as u16 },
        INPUT_KIND_KEY_UP => InputEvent::KeyUp { vk: a as u16 },
        INPUT_KIND_POINTER_DOWN => InputEvent::PointerDown {
            x: clamp_u16(a),
            y: clamp_u16(b),
        },
        INPUT_KIND_POINTER_UP => InputEvent::PointerUp {
            x: clamp_u16(a),
            y: clamp_u16(b),
        },
        INPUT_KIND_POINTER_MOVE => InputEvent::PointerMove {
            x: clamp_u16(a),
            y: clamp_u16(b),
        },
        other => {
            log::warn!("nativeSendInput: unknown kind {other}");
            return 0;
        }
    };
    session.send_input(InputCommand::Input(event));
    1
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativeRequestStop<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    if let Some(session) = session_from_handle(handle) {
        session.request_stop();
    }
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativeFinishGame<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jstring {
    if handle == 0 {
        return new_jstring(&env, "(no session)");
    }
    // Reclaim ownership and join. The handle is now invalid for
    // any subsequent call from Kotlin.
    let session = unsafe { Box::from_raw(handle as *mut Session) };
    let summary = session.finish();
    let mut sessions = LIBRARY_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    *sessions = sessions.saturating_sub(1);
    new_jstring(&env, summary)
}

/// Borrow the session pointer without consuming it.
fn session_from_handle<'a>(handle: jlong) -> Option<&'a Session> {
    if handle == 0 {
        return None;
    }
    // SAFETY: the handle was produced by `Box::into_raw` in
    // `nativeStartGame` and is freed exactly once by
    // `nativeFinishGame`. Kotlin guarantees the lifetime by treating
    // the handle as opaque and never passing a freed value back in.
    Some(unsafe { &*(handle as *const Session) })
}

fn clamp_u16(v: jint) -> u16 {
    v.clamp(0, u16::MAX as jint) as u16
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_renameGame<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    root: JString<'local>,
    id: JString<'local>,
    name: JString<'local>,
) -> jstring {
    let result = (|| -> anyhow::Result<()> {
        let root =
            jstring_to_path(&mut env, root).ok_or_else(|| anyhow::anyhow!("missing root"))?;
        let id: String = env.get_string(&id)?.into();
        let name: String = env.get_string(&name)?.into();
        Library::open(root)?.rename_game(&id, &name)?;
        Ok(())
    })();
    new_jstring(
        &env,
        match result {
            Ok(()) => "{\"ok\":true}".into(),
            Err(e) => error_json(&e.to_string()),
        },
    )
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_isGizmondoGame<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    root: JString<'local>,
    id: JString<'local>,
) -> jni::sys::jboolean {
    let result = (|| -> anyhow::Result<bool> {
        let root =
            jstring_to_path(&mut env, root).ok_or_else(|| anyhow::anyhow!("missing root"))?;
        let id: String = env.get_string(&id)?.into();
        let lib = Library::open(&root)?;
        Ok(lib
            .games()
            .iter()
            .find(|e| e.id == id)
            .map(|e| pocket_library::is_gizmondo_game(e, &root))
            .unwrap_or(false))
    })();
    u8::from(result.unwrap_or(false))
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_isColorsGame<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    root: JString<'local>,
    id: JString<'local>,
) -> jni::sys::jboolean {
    let result = (|| -> anyhow::Result<bool> {
        let root =
            jstring_to_path(&mut env, root).ok_or_else(|| anyhow::anyhow!("missing root"))?;
        let id: String = env.get_string(&id)?.into();
        let lib = Library::open(&root)?;
        Ok(lib
            .games()
            .iter()
            .find(|e| e.id == id)
            .map(|e| pocket_library::is_colors_game(e, &root))
            .unwrap_or(false))
    })();
    u8::from(result.unwrap_or(false))
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_upscaleXbrz<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    rgba: jni::objects::JByteArray<'local>,
    width: jint,
    height: jint,
) -> jbyteArray {
    if width <= 0 || height <= 0 || width > 2048 || height > 2048 {
        return std::ptr::null_mut();
    }
    let Ok(bytes) = env.convert_byte_array(rgba) else {
        return std::ptr::null_mut();
    };
    if bytes.len() != width as usize * height as usize * 4 {
        return std::ptr::null_mut();
    }
    let scaled = xbrz::scale_rgba(&bytes, width as usize, height as usize, 3);
    env.byte_array_from_slice(&scaled)
        .map(|a| a.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Reuse the desktop's analyzer; never run the executable to inspect metadata.
#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_analyzeGame<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    library_root: JString<'local>,
    game_id: JString<'local>,
) -> jstring {
    init_logger();
    let root = match jstring_to_path(&mut env, library_root) {
        Some(p) => p,
        None => return new_jstring(&env, error_json("missing library root")),
    };
    let id = match env.get_string(&game_id) {
        Ok(s) => s.to_string_lossy().into_owned(),
        Err(_) => return new_jstring(&env, error_json("missing game id")),
    };
    let result = (|| -> anyhow::Result<String> {
        let lib = Library::open(&root)?;
        let game = lib
            .games()
            .iter()
            .find(|game| game.id == id)
            .ok_or_else(|| anyhow::anyhow!("game not found: {id}"))?;
        let mut analysis = pocket_library::analyze_game(game, &root);
        if !pocket_library::is_gizmondo_game(game, &root) {
            analysis.dependencies =
                pocket_core::dependencies::information(&game.executable_path(&root), &root);
        }
        Ok(serde_json::to_string(&analysis)?)
    })();
    match result {
        Ok(json) => new_jstring(&env, json),
        Err(error) => new_jstring(&env, error_json(&format!("analyzeGame: {error:#}"))),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_isDemoCard<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    root: JString<'local>,
    id: JString<'local>,
) -> jni::sys::jboolean {
    let Some(root) = jstring_to_path(&mut env, root) else {
        return 0;
    };
    let Some(id) = jstring_to_string(&mut env, id) else {
        return 0;
    };
    u8::from(Library::open(root).is_ok_and(|lib| lib.is_demo_card(&id)))
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_splitDemoCard<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    root: JString<'local>,
    id: JString<'local>,
) -> jstring {
    let Some(root) = jstring_to_path(&mut env, root) else {
        return new_jstring(&env, error_json("missing library root"));
    };
    let Some(id) = jstring_to_string(&mut env, id) else {
        return new_jstring(&env, error_json("missing game ID"));
    };
    let result = (|| -> anyhow::Result<String> {
        let entries = Library::open(root)?.split_demo_card(&id)?;
        Ok(serde_json::to_string(&entries)?)
    })();
    new_jstring(&env, result.unwrap_or_else(|e| error_json(&e.to_string())))
}

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativePollDependencyNotice<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jstring {
    let Some(message) = session_from_handle(handle).and_then(|s| s.poll_dependency_notice()) else {
        return std::ptr::null_mut();
    };
    env.new_string(message)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Android exposes timer queries through EGL, but its Java GLES facade has no
/// 64-bit query-result entry point. The GL-thread caller checks availability.
#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativeGpuQueryResult(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    query: jint,
) -> jlong {
    #[cfg(target_os = "android")]
    {
        type GetResult = unsafe extern "system" fn(u32, u32, *mut u64);
        #[link(name = "EGL")]
        unsafe extern "C" {
            fn eglGetProcAddress(name: *const std::ffi::c_char) -> *const std::ffi::c_void;
        }
        static GET: std::sync::OnceLock<Option<GetResult>> = std::sync::OnceLock::new();
        let get = GET.get_or_init(|| unsafe {
            let p = eglGetProcAddress(c"glGetQueryObjectui64vEXT".as_ptr());
            if p.is_null() {
                None
            } else {
                Some(std::mem::transmute::<*const std::ffi::c_void, GetResult>(p))
            }
        });
        if let Some(get) = get {
            if query == 0 {
                return 0;
            }
            let mut ns = 0;
            unsafe {
                get(query as u32, 0x8866, &mut ns);
            }
            return i64::try_from(ns).unwrap_or(-1);
        }
    }
    #[cfg(not(target_os = "android"))]
    let _ = query;
    -1
}
