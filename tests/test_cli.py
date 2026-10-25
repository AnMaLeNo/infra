"""`infra set` : valeurs liste/dict (`data=[…]`, `entries={…}`) parsées en YAML, pas gardées en chaîne."""
from typer.testing import CliRunner

from infra import config
from infra.cli import app


def test_set_parses_list_and_dict(env):
    r = CliRunner().invoke(app, ["set", "fan-controller", "entries={fan-admin: public}", "data=[api:/data]", "port=8080"])
    assert r.exit_code == 0, r.output
    prj = config.registry().projects["fan-controller"]
    assert prj.entries == {"fan-admin": "public"}
    assert prj.data == ["api:/data"]
    assert prj.port == 8080
