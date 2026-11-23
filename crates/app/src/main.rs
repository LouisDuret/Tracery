//! Application de bureau Tracery (eframe / egui, rendu wgpu).
#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod doc;
mod selection;
mod settings;
mod worker;

use std::path::PathBuf;

/// Journal des erreurs fatales : `%APPDATA%/Tracery/erreur.log`.
fn log_path() -> PathBuf {
    settings::config_dir().join("erreur.log")
}

/// Sans console (sous-système « windows »), une erreur serait invisible :
/// on l'écrit dans le journal et on l'affiche dans une boîte de dialogue.
fn report(msg: &str) {
    let p = log_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(&p, msg);
    eprintln!("{msg}");
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("Tracery — erreur")
        .set_description(format!("{msg}\n\n(Journal : {})", p.display()))
        .show();
}

fn options(renderer: eframe::Renderer) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1500.0, 950.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("Tracery"),
        renderer,
        // carte graphique la plus puissante (dédiée si présente) ; la
        // variable WGPU_POWER_PREF permet encore de choisir
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(
                eframe::egui_wgpu::WgpuSetupCreateNew {
                    power_preference: eframe::wgpu::PowerPreference::from_env()
                        .unwrap_or(eframe::wgpu::PowerPreference::HighPerformance),
                    ..eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle()
                },
            ),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn main() {
    settings::migrate_old_dir();
    std::panic::set_hook(Box::new(|info| {
        // Les paniques des tâches de génération sont rattrapées par le
        // worker : on ne signale que celles du fil principal.
        if std::thread::current().name() == Some("main") {
            report(&format!("Plantage : {info}"));
        }
    }));
    let r = eframe::run_native(
        "Tracery",
        options(eframe::Renderer::Wgpu),
        Box::new(|cc| Ok(Box::new(app::TraceryApp::new(cc)))),
    );
    if let Err(e) = r {
        report(&format!("Impossible de démarrer l'interface : {e}"));
    }
}
