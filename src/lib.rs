//! infra : déploiement déclaratif piloté par Claude sur un hôte unique. Bibliothèque partagée par le binaire et les tests.
pub mod compose;
pub mod config;
pub mod dns;
pub mod history;
pub mod models;
pub mod paths;
pub mod previews;
pub mod render;
pub mod secrets;
pub mod skills;
pub mod stack;
pub mod state;
pub mod tunnel;
pub mod util;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
