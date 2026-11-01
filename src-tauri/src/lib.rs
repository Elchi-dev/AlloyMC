mod alloymc;
mod cfwindow;
mod doctor;
mod packs;
mod commands;
mod content;
mod jobs;
mod ms_login;
mod secrets;
mod state;
mod updater;

use std::sync::Arc;

use alloy_core::auth::{Accounts, FileSecretStore};
use alloy_core::paths::Dirs;
use alloy_core::Launcher;
use tauri::{Emitter, Manager};

fn data_dirs() -> Dirs {
    std::env::var("ALLOY_DATA_DIR").map(Dirs::new).unwrap_or_else(|_| Dirs::platform_default())
}

/// `alloy --fetch-assets [dir]`: what the Windows installer and the Arch package run at
/// install time to pull Alloy's block textures from Mojang. No window is opened.
fn fetch_assets_cli(args: &[String]) -> bool {
    let Some(pos) = args.iter().position(|a| a == "--fetch-assets") else {
        return false;
    };
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let code = rt.block_on(async {
        let launcher = Launcher::open(data_dirs()).await?;
        match args.get(pos + 1) {
            Some(dir) => alloy_core::textures::fetch_into(&launcher.http, &launcher, std::path::Path::new(dir)).await?,
            None => {
                alloy_core::textures::ensure(&launcher).await?;
            }
        }
        Ok::<_, alloy_core::Error>(())
    });
    match code {
        Ok(()) => println!("Alloy textures installed."),
        Err(e) => {
            eprintln!("Could not fetch textures: {e}");
            std::process::exit(1);
        }
    }
    true
}

pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    if fetch_assets_cli(&args) {
        return;
    }

    // WebKitGTK's DMA-BUF renderer shows a blank window on many NVIDIA + Wayland setups.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,alloy_core=debug".into()))
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let launcher = tauri::async_runtime::block_on(Launcher::open(data_dirs()))?;
            let file_store = FileSecretStore::new(launcher.dirs.root().join("secrets.json"));
            let secret_store: Arc<dyn alloy_core::auth::SecretStore> = Arc::new(secrets::KeyringStore::new(file_store));
            let accounts = Accounts::new(launcher.clone(), secret_store.clone());
            let alloymc = alloy_core::alloymc::AlloyMc::new(launcher.clone(), secret_store);
            app.manage(state::AppState::new(launcher.clone(), accounts, alloymc));
            app.manage(alloymc::SignIn::default());
            app.manage(cfwindow::CfQueue::default());
            app.manage(jobs::Jobs::default());
            doctor::recover_on_start(launcher.clone());
            updater::start_auto(app.handle().clone());
            // Development only: open the CurseForge window for an instance at start.
            #[cfg(debug_assertions)]
            if let Ok(id) = std::env::var("ALLOY_DEV_CURSEFORGE_WINDOW") {
                let h = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    if let Err(e) = cfwindow::open(&h, id).await {
                        tracing::error!("dev CurseForge window: {e}");
                    }
                });
            }
            jobs::start_speed_meter(app.handle().clone());
            content::start_blocked_watcher(app.handle().clone());

            // Normally the installer already fetched the textures. Portable or dev builds
            // fetch them here in the background; the UI swaps them in when they arrive.
            if alloy_core::textures::installed_dir(&launcher).is_none() {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    match alloy_core::textures::ensure(&launcher).await {
                        Ok(_) => {
                            let _ = handle.emit("textures-ready", ());
                        }
                        Err(e) => tracing::warn!("texture fetch failed: {e}"),
                    }
                });
            }

            // Safety net: the UI shows the window after its first paint; if that never
            // happens (a frontend crash), show it anyway so the app isn't invisible.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                if let Some(w) = handle.get_webview_window("main") {
                    if !w.is_visible().unwrap_or(true) {
                        let _ = w.show();
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::get_settings,
            commands::save_settings,
            commands::get_textures,
            commands::list_game_versions,
            commands::list_loader_versions,
            commands::fabric_api_version,
            commands::list_java,
            commands::list_instances,
            commands::get_instance,
            commands::create_instance,
            commands::update_instance,
            commands::delete_instance,
            commands::duplicate_instance,
            commands::install_instance,
            commands::open_instance_folder,
            commands::prewarm_launch,
            commands::launch_instance,
            commands::stop_instance,
            commands::running_instances,
            commands::list_accounts,
            commands::add_offline_account,
            commands::login_microsoft,
            commands::remove_account,
            commands::set_active_account,
            alloymc::alloymc_profile,
            alloymc::alloymc_sign_in,
            alloymc::alloymc_cancel_sign_in,
            alloymc::alloymc_sign_out,
            alloymc::alloymc_link_minecraft,
            alloymc::alloymc_unlink_minecraft,
            content::search_content,
            content::modrinth_project,
            content::modrinth_versions,
            content::modrinth_dependencies,
            content::curseforge_project,
            content::curseforge_description,
            content::curseforge_files,
            content::install_modrinth_version,
            content::install_curseforge_file,
            content::install_modrinth_modpack,
            content::install_curseforge_modpack,
            content::import_modpack,
            content::content_overview,
            content::content_icon,
            content::set_content_enabled,
            content::remove_content,
            content::content_fixes,
            content::apply_fix,
            content::check_content_updates,
            content::apply_content_updates,
            content::preflight,
            content::cleanup_orphans,
            content::fetch_listed_files,
            content::blocked_files,
            content::dismiss_blocked,
            content::resolve_link,
            content::add_files,
            content::set_instance_icon,
            content::instance_icon,
            content::read_image_file,
            content::memory_advice,
            content::list_jobs,
            content::cancel_job,
            doctor::crash_last,
            doctor::crash_dismiss,
            doctor::crash_apply_fix,
            doctor::bisect_start,
            packs::savepoints,
            packs::savepoint_create,
            packs::savepoint_restore,
            packs::savepoint_rename,
            packs::savepoint_delete,
            packs::export_candidates,
            packs::export_instance,
            packs::import_discover,
            packs::import_inspect,
            packs::import_instance,
            packs::pack_source,
            cfwindow::curseforge_window,
            content::clear_jobs,
            content::retry_job,
            updater::update_check_now,
            updater::check_updates,
            updater::inspect_update,
            updater::update_outcome,
            updater::apply_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Alloy");
}
