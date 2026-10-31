//! infra : déploiement déclaratif piloté par Claude sur un hôte unique. Bibliothèque partagée par le binaire et les tests.
pub mod agents;
pub mod api;
pub mod cli;
pub mod compose;
pub mod config;
pub mod dns;
pub mod doctor;
pub mod editor;
pub mod gitsync;
pub mod history;
pub mod mcp;
pub mod models;
pub mod observations;
pub mod paths;
pub mod previews;
pub mod render;
pub mod runner;
pub mod secrets;
pub mod skills;
pub mod stack;
pub mod state;
pub mod tools;
pub mod tunnel;
pub mod util;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
