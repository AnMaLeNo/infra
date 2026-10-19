from infra import paths, stack


def test_host_stack_overrides_tool(env):
    d = paths.config_dir() / "stack" / "pont"
    d.mkdir(parents=True)
    (d / "docker-compose.yml").write_text("services: {}\n")
    assert stack.stack_dir("pont") == d
    assert stack.stack_dir("traefik") == paths.tool_dir() / "stack" / "traefik"


def test_sync_dynamic_merges_tool_and_host(env):
    host_dyn = paths.config_dir() / "stack" / "traefik" / "dynamic"
    host_dyn.mkdir(parents=True)
    (host_dyn / "ibkr.yml").write_text("http: {}\n")
    dst = env / "dyn"
    (dst.mkdir(), (dst / "stale.yml").write_text("x"))
    stack._sync_dynamic(dst)
    names = sorted(p.name for p in dst.glob("*.yml"))
    assert names == ["dashboard.yml", "ibkr.yml"]


def test_render_static_replaces_env_templates():
    src = "email: \"{{ env `ACME_EMAIL` }}\"\nmain: \"{{ env `PRIVATE_ZONE` }}\"\nsans: [\"*.{{ env `PRIVATE_ZONE` }}\"]\n"
    out = stack.render_static(src, {"ACME_EMAIL": "a@b.c", "PRIVATE_ZONE": "pi.example.com"})
    assert out == 'email: "a@b.c"\nmain: "pi.example.com"\nsans: ["*.pi.example.com"]\n'
    assert "{{" not in out
