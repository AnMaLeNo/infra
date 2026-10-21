import yaml

from infra import config, skills
from infra.render import Target, build_override, traefik_labels

COMPOSE = {
    "services": {
        "frontend": {"image": "x", "ports": ["18081:80"]},
        "api": {"build": ".", "ports": ["3000:3000"], "networks": ["back"]},
        "db": {"image": "postgres", "volumes": ["pgdata:/var/lib/postgresql/data"]},
    },
    "networks": {"back": {}},
    "volumes": {"pgdata": {}},
}


def test_private_override(env):
    h, reg = config.host(), config.registry()
    t = Target("fan-controller", reg.projects["fan-controller"], h)
    ov = build_override(t, COMPOSE)
    for svc in COMPOSE["services"]:
        assert ov["services"][svc]["ports"] == []          # !reset
    fe = ov["services"]["frontend"]
    assert "traefik-net" in fe["networks"] and "default" in fe["networks"]
    labels = fe["labels"]
    assert "traefik.http.routers.fan-controller-private.rule=Host(`fan-controller.pi.antoine42.com`)" in labels
    assert not any("-public." in lab for lab in labels)
    assert "traefik.http.services.fan-controller.loadbalancer.server.port=80" in labels
    assert "labels" not in ov["services"]["api"]


def test_public_tunnel_labels(env):
    h, reg = config.host(), config.registry()
    t = Target("catacombes", reg.projects["catacombes"], h)
    labels = traefik_labels(t)
    assert "traefik.http.routers.catacombes-public.entrypoints=tunnel" in labels
    assert "traefik.http.routers.catacombes-private.entrypoints=tailnet" in labels
    assert t.urls == {"private": "https://catacombes.pi.antoine42.com", "public": "https://catacombes.antoine42.com"}


def test_preview_target(env):
    h, reg = config.host(), config.registry()
    t = Target("site-client", reg.projects["site-client"], h, pr=12)
    assert t.compose_project == "site-client-pr-12"
    assert t.public_fqdn == "pr-12.naturo.antoine42.com"
    assert "traefik.http.routers.site-client-pr-12-public.rule=Host(`pr-12.naturo.antoine42.com`)" in traefik_labels(t)


def test_none_has_no_labels(env):
    h, reg = config.host(), config.registry()
    t = Target("unused", reg.projects["unused"], h)
    ov = build_override(t, {"services": {"daemon": {"image": "x", "ports": ["1:1"]}}})
    assert "labels" not in ov["services"]["daemon"] and "networks" not in ov


def test_reset_tag_serialised(env):
    from infra.render import ResetList, _Dumper
    out = yaml.dump({"ports": ResetList()}, Dumper=_Dumper)
    assert "!reset []" in out


def test_skills_resolution(env):
    reg = config.registry()
    assert skills.for_project(reg.projects["site-client"]) == ["service/static", "expose/public", "previews"]
    assert skills.for_project(reg.projects["immich"]) == ["undeploy"]
    # service absent : les deux skills service/*, le LLM choisit
    assert skills.for_project(reg.projects["unused"]) == ["service/compose", "service/static", "expose/none"]
    assert all(p is not None for p in skills.paths_for(reg.projects["catacombes"]).values())


def test_set_project_fields(env):
    config.set_project_fields("catacombes", port=9000, notes="x")
    assert config.project("catacombes").port == 9000


def test_llm_decided_fields_do_not_mark_project_changed(env):
    from infra import runner

    before = runner._hash(config.project("catacombes"))
    config.set_project_fields("catacombes", container="api", port=1234, build="make", source="llm")
    assert runner._hash(config.project("catacombes")) == before
    assert config.project("catacombes").decided() == {"container": "api", "port": 1234, "build": "make"}
    config.set_project_fields("catacombes", expose="private")
    assert runner._hash(config.project("catacombes")) != before



def test_changed_since_last_run_and_queue(env):
    from infra import runner
    state = runner.load_state()
    assert set(runner.changed_since_last_run(state)) == set(config.registry().projects)   # jamais lancés
    for n, prj in config.registry().projects.items():
        runner.mark_applied(state, n, prj, ok=True)
    assert runner.changed_since_last_run(runner.load_state()) == []
    config.set_project_fields("catacombes", domain="cata")  # une intention ; `port` seul ne compterait pas (LLM_DECIDED)
    assert runner.changed_since_last_run(runner.load_state()) == ["catacombes"]
    runner.enqueue("catacombes")
    runner.enqueue("catacombes")
    assert runner.queued() == ["catacombes"]
    rec = runner.process_one(dry=True)                      # dry : construit le prompt, ne lance pas claude
    assert rec["project"] == "catacombes" and rec["dry"] and runner.queued() == [] and runner.running() is None
    assert runner.process_one(dry=True) is None
