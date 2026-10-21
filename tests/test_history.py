import json

from infra import config, history, runner


def test_history_records_config_versions(env):
    config.set_project_fields("site-client", source="test", expose="public")
    config.set_project_fields("site-client", source="test", expose="public")  # identique → pas de doublon
    config.set_project_fields("site-client", source="test", expose="private")
    h = history.for_project("site-client")
    assert [e["entry"]["expose"] for e in h] == ["public", "private"]
    assert h[0]["source"] == "test"
    config.remove_project("site-client", source="test")
    assert history.for_project("site-client")[-1]["entry"] is None


def test_llm_settings_and_prompt_record(env):
    assert config.host().llm.model_dump() == {"model": "sonnet", "effort": "high"}
    config.set_llm(model="opus", effort="max")
    assert config.host().llm.model == "opus"
    import pytest
    with pytest.raises(ValueError):
        config.set_llm(effort="extreme")
    runner.enqueue("unused")
    rec = runner.process_one(dry=True)
    assert rec["project"] == "unused"
    runs = runner.runs_for_project("unused")
    assert runs == []  # dry : rien d'écrit sur disque


def test_runs_for_project_and_log(env):
    rec = {"project": "unused", "started": "2026-10-17T10:00:00+00:00", "ok": True, "model": "sonnet", "effort": "high",
           "result": "fait"}
    p = runner._save(rec)
    p.with_suffix(".log").write_text(json.dumps({"type": "result", "result": "fait"}))
    runs = runner.runs_for_project("unused")
    assert len(runs) == 1 and runs[0]["log"] == p.with_suffix(".log").name and runs[0]["model"] == "sonnet"
    assert "fait" in runner.get_log(runs[0]["log"])
    assert runner.runs_for_project("site-client") == []


def test_final_status_parsing():
    from infra.runner import final_status

    assert final_status("blabla\n\nÉTAT FINAL : ok") == "ok"
    assert final_status("…\n**ÉTAT FINAL : bloqué**") == "bloqué"
    assert final_status("ÉTAT FINAL : ok\nfinalement non\nÉTAT FINAL : échec") == "échec"
    assert final_status("rien") == "inconnu"


def test_git_env_with_and_without_token(env, monkeypatch):
    from infra import compose, secrets

    monkeypatch.setattr(secrets, "infra_env", lambda: {})
    assert compose.git_env() == {"GIT_TERMINAL_PROMPT": "0"}
    monkeypatch.setattr(secrets, "infra_env", lambda: {"GITHUB_TOKEN": "ghp_x"})
    e = compose.git_env()
    assert e["GH_TOKEN"] == "ghp_x" and e["GIT_CONFIG_VALUE_0"] == "https://github.com/"
    assert "x-access-token:ghp_x@github.com" in e["GIT_CONFIG_KEY_0"]
