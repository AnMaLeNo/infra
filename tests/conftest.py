import shutil
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent


@pytest.fixture
def env(tmp_path, monkeypatch):
    cfg = tmp_path / "config"
    shutil.copytree(ROOT / "example-config", cfg)
    (cfg / "tool").symlink_to(ROOT)
    for k, d in {"INFRA_CONFIG_DIR": cfg, "INFRA_APPS_DIR": tmp_path / "apps",
                 "INFRA_DATA_DIR": tmp_path / "data", "INFRA_STACK_DIR": tmp_path / "stack"}.items():
        monkeypatch.setenv(k, str(d))
    return tmp_path
