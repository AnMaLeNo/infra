"""skillforge côté infra : adaptateur run → Episode, mode off sans effet, indisponible sans clé, skill custom prioritaire."""
import json
import shutil
from pathlib import Path

import pytest

from infra import config, forge, paths

FIX = Path(__file__).parent / "fixtures" / "forge"
BLOCKED = "2026-10-21T223808.142789+0000-catacombes"
OK = "2026-10-25T175343.887806+0000-naturopathe-website"


def _runs(env):
    d = paths.runs_dir()
    d.mkdir(parents=True, exist_ok=True)
    for f in FIX.iterdir():
        shutil.copy(f, d / f.name)
    return d


def test_episode_from_run(env):
    d = _runs(env)
    rec = json.loads((d / f"{BLOCKED}.json").read_text())
    ep = forge.episode(rec, (d / f"{BLOCKED}.log").read_text(), episode_id=BLOCKED)
    assert [s.id for s in ep.skills] == rec["skills"] and all("## But" in s.text for s in ep.skills)
    assert ep.trace.events and any(e.get("type") == "assistant" for e in ep.trace.events)
    assert ep.agent_report.status == "bloqué" and "bascule" in ep.agent_report.needs_human.lower()


def test_mode_off_is_noop(env, monkeypatch):
    d = _runs(env)
    rec = json.loads((d / f"{OK}.json").read_text())
    assert config.host().forge.mode == "off"
    assert forge.judge(rec, (d / f"{OK}.log").read_text(), episode_id=OK) is None


def test_shadow_without_key_is_indisponible(env, monkeypatch):
    d = _runs(env)
    monkeypatch.delenv("OPENROUTER_API_KEY", raising=False)
    monkeypatch.setattr(forge.secrets, "infra_env", lambda: {})
    config.set_forge(mode="shadow")
    rec = json.loads((d / f"{OK}.json").read_text())
    v = forge.judge(rec, (d / f"{OK}.log").read_text(), episode_id=OK)
    assert v["decision"] == "indisponible" and "OPENROUTER_API_KEY" in v["errors"][0]
    assert rec.get("ok") is True  # l'issue du run n'est pas touchée


def test_replay_writes_verdict_and_custom_skill_wins(env, monkeypatch):
    d = _runs(env)
    monkeypatch.delenv("OPENROUTER_API_KEY", raising=False)
    monkeypatch.setattr(forge.secrets, "infra_env", lambda: {})
    custom = paths.config_dir() / "skills" / "expose" / "public" / "SKILL.md"
    custom.parent.mkdir(parents=True)
    custom.write_text("---\nname: expose/public\n---\n## But\ncustom\n")
    rec = json.loads((d / f"{BLOCKED}.json").read_text())
    ep = forge.episode(rec, "", episode_id=BLOCKED)
    assert next(s.text for s in ep.skills if s.id == "expose/public") == custom.read_text()
    v = forge.replay(f"{BLOCKED}.json")
    assert json.loads((d / f"{BLOCKED}.json").read_text())["forge"]["decision"] == v["decision"] == "indisponible"


def test_answer_requires_pending_question(env):
    _runs(env)
    with pytest.raises(ValueError):
        forge.answer(f"{OK}.json", "peu importe")


def test_judge_timeout_is_indisponible(env, monkeypatch):
    d = _runs(env)
    monkeypatch.setenv("OPENROUTER_API_KEY", "x")
    config.set_forge(mode="shadow")
    import time
    monkeypatch.setattr(forge, "episode", lambda *a, **k: time.sleep(5))
    rec = json.loads((d / f"{OK}.json").read_text())
    v = forge.judge(rec, "", episode_id=OK, timeout_s=0.2)
    assert v["decision"] == "indisponible" and "120" not in v["errors"][0] and "pas de verdict" in v["errors"][0]
