#![allow(dead_code)]
//! Fixture `env` : copie d'example-config dans un dossier temporaire + variables INFRA_* ; les variables d'env
//! étant globales au processus, les tests qui l'utilisent sont sérialisés par un verrou.
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use once_cell::sync::Lazy;

static LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

pub struct Env {
    pub root: tempfile::TempDir,
    _guard: MutexGuard<'static, ()>,
}

impl Env {
    pub fn config(&self) -> PathBuf {
        self.root.path().join("config")
    }
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
}

pub fn env() -> Env {
    let guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let root = tempfile::tempdir().unwrap();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cfg = root.path().join("config");
    copy_dir(&repo.join("example-config"), &cfg);
    std::os::unix::fs::symlink(&repo, cfg.join("tool")).unwrap();
    std::env::set_var("INFRA_CONFIG_DIR", &cfg);
    std::env::set_var("INFRA_APPS_DIR", root.path().join("apps"));
    std::env::set_var("INFRA_DATA_DIR", root.path().join("data"));
    std::env::set_var("INFRA_STACK_DIR", root.path().join("stack"));
    std::env::set_var("INFRA_SHARED_DIR", root.path().join("shared"));
    Env { root, _guard: guard }
}
