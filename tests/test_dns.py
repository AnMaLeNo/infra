from infra import config
from infra.dns import project_names, wanted_records


def test_wanted(env):
    h = config.host()
    h.cloudflare.tunnel_id = "abc"
    reg = config.registry()
    recs = wanted_records("site-client", reg.projects["site-client"], h)
    assert ("naturo.antoine42.com", "CNAME", "abc.cfargotunnel.com", True) in recs
    assert ("*.naturo.antoine42.com", "CNAME", "abc.cfargotunnel.com", True) in recs
    assert wanted_records("fan-controller", reg.projects["fan-controller"], h) == []
    assert "fan-controller.antoine42.com" in project_names("fan-controller", reg.projects["fan-controller"], h)


def test_entries_public_cname(env):
    h = config.host()
    h.cloudflare.tunnel_id = "abc"
    prj = config.set_project_fields("fan-controller", source="test", entries={"fan-admin": "public", "fan-ops": "private"})
    assert wanted_records("fan-controller", prj, h) == [("fan-admin.antoine42.com", "CNAME", "abc.cfargotunnel.com", True)]
    names = project_names("fan-controller", prj, h)
    assert "fan-admin.antoine42.com" in names and "fan-ops.antoine42.com" in names
