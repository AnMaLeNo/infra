//! infra : déploiement déclaratif piloté par Claude sur un hôte unique. Bibliothèque partagée par le binaire et les tests.
pub mod compose;
pub mod config;
pub mod history;
pub mod models;
pub mod paths;
pub mod render;
pub mod secrets;
pub mod skills;
pub mod state;
pub mod util;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
