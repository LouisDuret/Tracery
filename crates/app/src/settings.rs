//! Réglages de l'application conservés d'une session à l'autre (ADR-27) :
//! fichier JSON `reglages.json` dans le dossier de l'application, à côté de
//! la sauvegarde automatique.

use io::raster::Mode;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

fn config_base() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Dossier de l'application : `%APPDATA%/Tracery` (ou `~/.config/Tracery`).
pub fn config_dir() -> PathBuf {
    config_base().join("Tracery")
}

/// Reprise des données de l'ancien nom (ADR-28) : au premier lancement de
/// Tracery, les réglages et la session de `%APPDATA%/Remplage` sont copiés
/// (l'ancien dossier reste en place).
pub fn migrate_old_dir() {
    migrate_from(&config_base().join("Remplage"), &config_dir());
}

pub fn migrate_from(old: &Path, new: &Path) {
    if new.exists() || !old.is_dir() {
        return;
    }
    if std::fs::create_dir_all(new).is_err() {
        return;
    }
    for (a, b) in [
        ("reglages.json", "reglages.json"),
        ("autosave.remplage", "autosave.tracery"),
    ] {
        let _ = std::fs::copy(old.join(a), new.join(b));
    }
}

pub fn settings_path() -> PathBuf {
    config_dir().join("reglages.json")
}

/// Thème de l'interface (egui).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Interface {
    #[default]
    Systeme,
    Clair,
    Sombre,
}

impl Interface {
    pub fn to_egui(self) -> egui::ThemePreference {
        match self {
            Interface::Systeme => egui::ThemePreference::System,
            Interface::Clair => egui::ThemePreference::Light,
            Interface::Sombre => egui::ThemePreference::Dark,
        }
    }
    pub fn from_egui(t: egui::ThemePreference) -> Interface {
        match t {
            egui::ThemePreference::System => Interface::Systeme,
            egui::ThemePreference::Light => Interface::Clair,
            egui::ThemePreference::Dark => Interface::Sombre,
        }
    }
}

/// Réglages persistants. Un champ absent ou illisible garde sa valeur par
/// défaut : un fichier ancien ou abîmé ne bloque jamais le démarrage.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Mode d'affichage du canevas (clé de `Mode::parse`).
    pub mode: Option<String>,
    /// Thème de l'interface.
    pub interface: Interface,
    /// Fichier projet associé à la session (« Enregistrer » y écrit).
    pub projet: Option<PathBuf>,
}

impl Settings {
    pub fn load_from(path: &Path) -> Settings {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let t = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, t)
    }
    pub fn mode(&self) -> Option<Mode> {
        self.mode.as_deref().and_then(Mode::parse)
    }
}

/// Clé d'un mode, relue par `Mode::parse`.
pub fn mode_key(m: Mode) -> &'static str {
    match m {
        Mode::Trait => "trait",
        Mode::Pierre => "pierre",
        Mode::Epure => "epure",
        Mode::Vitrail => "vitrail",
        Mode::EpureSombre => "epure-sombre",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_relus() {
        for m in [
            Mode::Trait,
            Mode::Pierre,
            Mode::Epure,
            Mode::Vitrail,
            Mode::EpureSombre,
        ] {
            assert_eq!(Mode::parse(mode_key(m)), Some(m));
        }
    }

    #[test]
    fn aller_retour_fichier() {
        let dir = std::env::temp_dir().join(format!("tracery-test-{}", std::process::id()));
        let path = dir.join("reglages.json");
        let s = Settings {
            mode: Some(mode_key(Mode::EpureSombre).into()),
            interface: Interface::Sombre,
            projet: Some(PathBuf::from("C:/projets/baie.tracery")),
        };
        s.save_to(&path).unwrap();
        let r = Settings::load_from(&path);
        assert_eq!(r, s);
        assert_eq!(r.mode(), Some(Mode::EpureSombre));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fichier_absent_ou_abime() {
        let p = std::env::temp_dir().join("tracery-absent-xyz/reglages.json");
        assert_eq!(Settings::load_from(&p), Settings::default());
        let dir = std::env::temp_dir().join(format!("tracery-abime-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("reglages.json");
        std::fs::write(&p, "{ \"mode\": 3, \"interface\": \"clair\"").unwrap();
        assert_eq!(Settings::load_from(&p), Settings::default());
        // champ inconnu ou manquant : le reste est lu
        std::fs::write(&p, "{ \"interface\": \"clair\", \"futur\": 1 }").unwrap();
        assert_eq!(Settings::load_from(&p).interface, Interface::Clair);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reprise_ancien_dossier() {
        let base = std::env::temp_dir().join(format!("tracery-migr-{}", std::process::id()));
        let (old, new) = (base.join("Remplage"), base.join("Tracery"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("reglages.json"), "{ \"mode\": \"vitrail\" }").unwrap();
        std::fs::write(old.join("autosave.remplage"), "{}").unwrap();
        migrate_from(&old, &new);
        assert_eq!(
            Settings::load_from(&new.join("reglages.json")).mode(),
            Some(Mode::Vitrail)
        );
        assert!(new.join("autosave.tracery").is_file());
        assert!(
            old.join("reglages.json").is_file(),
            "l'ancien dossier reste en place"
        );
        // une seule fois : un dossier Tracery existant n'est pas écrasé
        std::fs::write(old.join("reglages.json"), "{ \"mode\": \"trait\" }").unwrap();
        migrate_from(&old, &new);
        assert_eq!(
            Settings::load_from(&new.join("reglages.json")).mode(),
            Some(Mode::Vitrail)
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
