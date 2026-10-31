//! Chemins conventionnels sur l'hôte (docs/conventions.md). Surchargeables par variables d'env pour les tests.
use std::env;
use std::path::PathBuf;

fn env_path(var: &str, default: &str) -> PathBuf {
    env::var_os(var).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(default))
}

pub fn config_dir() -> PathBuf {
    env_path("INFRA_CONFIG_DIR", "/srv/infra")
}

/// Dossier de l'outil (skills/, stack/, README.md, docs/) : `INFRA_TOOL_DIR`, sinon le sous-module `<config>/tool`,
/// sinon l'arbre source (développement), sinon l'installation système.
pub fn tool_dir() -> PathBuf {
    if let Some(p) = env::var_os("INFRA_TOOL_DIR") {
        return PathBuf::from(p);
    }
    let sub = config_dir().join("tool");
    if sub.exists() {
        return sub;
    }
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if src.join("skills").exists() {
        return src;
    }
    PathBuf::from("/usr/local/share/infra")
}

pub fn apps_dir() -> PathBuf {
    env_path("INFRA_APPS_DIR", "/srv/apps")
}

pub fn data_dir() -> PathBuf {
    env_path("INFRA_DATA_DIR", "/srv/data")
}

pub fn stack_state_dir() -> PathBuf {
    env_path("INFRA_STACK_DIR", "/srv/stack")
}

pub fn runs_dir() -> PathBuf {
    config_dir().join("runs")
}

pub fn age_key_file() -> PathBuf {
    env::var_os("SOPS_AGE_KEY_FILE").map(PathBuf::from).unwrap_or_else(|| config_dir().join(".age-key"))
}

pub fn app_dir(project: &str) -> PathBuf {
    apps_dir().join(project)
}

pub fn preview_dir(project: &str, pr: u64) -> PathBuf {
    app_dir(project).join("previews").join(format!("pr-{pr}"))
}

/// Définition d'un service partagé (compose + fichiers de conf), versionnée dans le repo de config.
pub fn shared_dir(name: &str) -> PathBuf {
    config_dir().join("shared").join(name)
}

/// Données/état d'un service partagé sur l'hôte (bind mounts) : ce qu'on sauvegarde.
pub fn shared_state_dir(name: &str) -> PathBuf {
    env_path("INFRA_SHARED_DIR", "/srv/shared").join(name)
}
