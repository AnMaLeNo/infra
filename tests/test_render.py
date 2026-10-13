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
    assert all(p is not None for p in skills.paths_for(reg.projects["catacombes"]).values())


def test_set_project_fields(env):
    config.set_project_fields("catacombes", port=9000, notes="x")
    assert config.project("catacombes").port == 9000
