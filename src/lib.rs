//! infra : déploiement déclaratif piloté par Claude sur un hôte unique. Bibliothèque partagée par le binaire et les tests.
pub mod config;
pub mod history;
pub mod models;
pub mod paths;
pub mod state;
pub mod util;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
