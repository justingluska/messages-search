//! Messages Search desktop: thin Tauri glue between the React UI
//! (apps/desktop/src, through src/lib/api.ts only) and ms-engine.

pub mod commands;
pub mod error;
pub mod files;
pub mod status;

use std::sync::Arc;

use ms_core::types::{Embedder, IndexPhase, IndexProgress};
use ms_embed::{DownloadEvent, EmbedOptions, FastEmbedder, ModelChoice};
use ms_engine::{Config, EmbedderLoader, Engine, Event, ModelRole, ProgressFn};
use tauri::{Emitter, Manager};

/// Starts the engine: index in the app's data dir, models under it.
fn start_engine(app: &tauri::AppHandle) -> Result<Arc<Engine>, Box<dyn std::error::Error>> {
    // Dev/demo overrides: point at a fixture chat.db and a separate data dir
    // (screenshots use fictional data, never a real history).
    let data = match std::env::var_os("MESSAGES_SEARCH_DATA") {
        Some(d) => std::path::PathBuf::from(d),
        None => app.path().app_data_dir()?,
    };
    let db_path = std::env::var_os("MESSAGES_SEARCH_DB")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(ms_source::default_db_path);
    let models = data.join("models");
    let loader: EmbedderLoader = Arc::new(move |role: ModelRole, progress: ProgressFn| {
        let choice = ModelChoice::default();
        // Background embedding uses 3 threads: measured about as fast as 9
        // at well under half the energy, and the Mac stays responsive.
        // Queries use ONNX's default (latency matters there).
        let options = EmbedOptions {
            intra_threads: match role {
                ModelRole::Passages => Some(3),
                ModelRole::Queries => None,
            },
        };
        let mb = choice.approx_download_bytes() / 1_000_000;
        let on_download: ms_embed::DownloadCallback = Box::new(move |e| {
            let (done, total) = match e {
                DownloadEvent::Started { total_bytes } => (0, total_bytes),
                DownloadEvent::Progress {
                    done_bytes,
                    total_bytes,
                } => (done_bytes, total_bytes),
                DownloadEvent::Finished { total_bytes } => (total_bytes, total_bytes),
            };
            progress(IndexProgress {
                phase: IndexPhase::DownloadingModel,
                done,
                total,
                message: Some(format!("Downloading search model ({mb} MB)")),
            });
        });
        let e = FastEmbedder::with_options(choice, &models, Some(on_download), options)
            .map_err(|e| e.to_string())?;
        Ok(Arc::new(e) as Arc<dyn Embedder>)
    });

    let handle = app.clone();
    let engine = Engine::start(
        Config {
            db_path,
            index_path: data.join("index.db"),
            tz: ms_core::Tz::System,
            embedder: Some(loader),
            // Not for fixture/demo runs: their names come from the fixture.
            use_contacts: std::env::var_os("MESSAGES_SEARCH_DB").is_none(),
        },
        move |event| {
            let _ = match event {
                Event::Progress(p) => handle.emit(status::EVENT_INDEX_PROGRESS, p),
                Event::Changed => handle.emit(status::EVENT_INDEX_CHANGED, ()),
            };
        },
    )?;
    Ok(engine)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // One copy at a time: a second launch focuses the running window
        // instead of starting another indexer on the same index.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let engine = start_engine(app.handle())?;
            app.manage(engine);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::search,
            commands::messages_around,
            commands::messages_page,
            commands::get_chat,
            commands::list_chats,
            commands::open_in_messages,
            commands::open_full_disk_access_settings,
            commands::reveal_attachment,
            commands::reindex,
            commands::open_contacts_settings,
            commands::reveal_index,
            commands::app_version,
            commands::insights,
            commands::storage_summary,
            commands::list_attachments,
            commands::open_attachment,
            commands::save_attachment,
            commands::copy_attachment,
            commands::trash_attachments,
            commands::open_storage_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Messages Search");
}
