"""Services partagés (shared.yaml) : modèle, consommation par les projets (`uses` / `consumers`), runs, API."""
import pytest
import yaml
from fastapi.testclient import TestClient

from infra import config, paths, runner, skills, tools
from infra.api import app
from infra.render import Target, build_override

COMPOSE = {"services": {"frontend": {"image": "x", "ports": ["18081:5173"]},
                        "api": {"build": ".", "networks": ["iot_net"], "environment": ["PORT=3000"]},
                        "mosquitto": {"image": "eclipse-mosquitto:2", "ports": ["1883:1883"]}},
           "networks": {"iot_net": {}}}


def test_shared_declared_with_users_computed(env):
    svc = config.shared_service("mqtt")
    assert svc.kind == "mqtt-broker" and svc.listen == ["lan", "tailnet"] and svc.port == 1883
    assert config.users_of("mqtt") == ["fan-controller"]
    config.set_project_fields("catacombes", uses=["mqtt"])
    assert config.users_of("mqtt") == ["catacombes", "fan-controller"]
    assert tools.shared_get("mqtt")["mqtt"]["users"] == ["catacombes", "fan-controller"]


def test_consumers_wired_to_shared_service(env):
    t = Target("fan-controller", config.project("fan-controller"), config.host())
    ov = build_override(t, COMPOSE)
    api = ov["services"]["api"]
    assert api["environment"] == {"MQTT_HOST": "mqtt", "MQTT_PORT": "1883"}
    assert api["networks"] == ["iot_net", "traefik-net"]
    assert ov["networks"] == {"traefik-net": {"external": True}}
    assert ov["services"]["mosquitto"]["ports"] == []          # la copie du projet ne publie rien


def test_uses_undeclared_blocks_instead_of_creating(env):
    config.set_project_fields("fan-controller", uses=["redis"], consumers={"api": {"REDIS_URL": "redis://redis"}})
    t = Target("fan-controller", config.project("fan-controller"), config.host())
    with pytest.raises(ValueError, match="non déclarés dans shared.yaml : \\['redis'\\]"):
        build_override(t, COMPOSE)


def test_uses_without_consumers_asks_for_them(env):
    config.set_project_fields("fan-controller", consumers=None)
    t = Target("fan-controller", config.project("fan-controller"), config.host())
    with pytest.raises(ValueError, match="`consumers` est vide"):
        build_override(t, COMPOSE)


def test_consumers_is_llm_decided_not_intent(env):
    state = runner.load_state()
    prj = config.project("fan-controller")
    runner.mark_applied(state, "fan-controller", prj, ok=True)
    config.set_project_fields("fan-controller", consumers={"api": {"MQTT_HOST": "mqtt"}})
    assert "fan-controller" not in runner.changed_since_last_run(runner.load_state())
    config.set_project_fields("fan-controller", uses=["mqtt", "db"])
    assert "fan-controller" in runner.changed_since_last_run(runner.load_state())


def test_shared_set_only_findings(env):
    out = tools.shared_set("mqtt", {"image": "eclipse-mosquitto:2.1-alpine", "container": "mqtt_broker"})
    assert out["mqtt"]["image"] == "eclipse-mosquitto:2.1-alpine"
    assert config.shared_service("mqtt").decided() == {"image": "eclipse-mosquitto:2.1-alpine", "container": "mqtt_broker"}
    with pytest.raises(ValueError, match="choix humains"):
        tools.shared_set("mqtt", {"port": 1884})
    with pytest.raises(KeyError, match="inconnu"):
        tools.shared_set("redis", {"image": "redis:7"})
    hist = (paths.runs_dir() / "history" / "shared.mqtt.jsonl").read_text().splitlines()
    assert len(hist) == 1 and '"source": "llm"' in hist[0]


def test_remove_shared_refused_while_used(env):
    with pytest.raises(ValueError, match="encore utilisé par : fan-controller"):
        config.remove_shared("mqtt")
    config.set_project_fields("fan-controller", uses=[])
    config.remove_shared("mqtt")
    assert config.shared().services == {}


def test_shared_run_queued_and_dry(env):
    assert skills.resolve("shared-service") is not None
    runner.enqueue(config.shared_key("mqtt"))
    assert runner.queued() == ["shared.mqtt"]
    rec = runner.process_one(dry=True)
    assert rec["project"] == "shared.mqtt" and rec["skills"] == ["shared-service"] and rec["dry"]
    assert "SERVICE PARTAGÉ `mqtt`" in rec["prompt"] and "fan-controller" in rec["prompt"]
    assert "run_report(project='shared.mqtt'" in rec["prompt"]
    assert "listen:\n  - lan\n  - tailnet" in rec["prompt"] or "listen: [lan, tailnet]" in yaml.safe_dump(rec["after"])
    assert (paths.runs_dir() / f"{rec['started'].replace(':', '').replace('+00:00', 'Z')}-shared.mqtt.json").exists() is False  # dry : pas sauvegardé
    assert runner.runs_for_project("shared.mqtt") == []


def test_shared_hash_ignores_findings(env):
    state = runner.load_state()
    runner.mark_applied(state, "shared.mqtt", config.shared_service("mqtt"), ok=True)
    config.set_shared_fields("mqtt", "llm", image="eclipse-mosquitto:2.1-alpine")
    assert "shared.mqtt" not in runner.changed_since_last_run(runner.load_state())
    config.set_shared_fields("mqtt", port=1884)
    assert "shared.mqtt" in runner.changed_since_last_run(runner.load_state())


def test_mcp_shared_tools_registered():
    import asyncio

    from infra.mcp_server import server
    names = {t.name for t in asyncio.run(server.list_tools())}
    assert {"shared_get", "shared_set", "shared_status", "host_addresses"} <= names


def test_api_shared(env):
    c = TestClient(app)
    r = c.get("/api/shared").json()
    assert r["mqtt"]["users"] == ["fan-controller"] and r["mqtt"]["never_run"] and r["mqtt"]["key"] == "shared.mqtt"
    assert c.get("/api/host").json()["lan_ip"] == "192.168.1.101"
    r = c.put("/api/shared/pg", json={"kind": "postgres", "listen": ["tailnet"], "port": None, "auth": "users", "data": True,
                                      "enabled": True, "notes": ""})
    assert r.status_code == 200 and r.json()["kind"] == "postgres"
    assert c.put("/api/shared/bad", json={"kind": "x", "listen": ["wan"]}).status_code == 422
    assert c.post("/api/shared/pg/run", json={}).json()["project"] == "shared.pg"
    assert c.get("/api/shared").json()["pg"]["queued"]
    assert c.delete("/api/shared/mqtt").status_code == 409          # consommé par fan-controller
    assert c.delete("/api/shared/pg").status_code == 200
    h = c.get("/api/shared/pg/history").json()
    assert [e["entry"] is None for e in h["config"]] == [False, True]


def test_shared_notes_ignored_and_changed_fields(env):
    from infra import runner

    state = runner.load_state()
    runner.mark_applied(state, "shared.mqtt", config.shared_service("mqtt"), ok=True)
    config.set_shared_fields("mqtt", notes="2 ESP sur le LAN")
    assert "shared.mqtt" not in runner.changed_since_last_run(runner.load_state())
    config.set_shared_fields("mqtt", port=1884)
    assert runner.changed_fields(config.shared_service("mqtt"), state["shared.mqtt"]) == ["port"]
