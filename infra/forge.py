"""Client de skillforge : un run → Episode, verdict dans runs/<run>.json, réponses de l'auteur → historique + patch.

skillforge ne connaît pas infra ; tout passe par son contrat (Episode/Verdict). `host.yaml → forge.mode` :
off (rien), shadow (verdict écrit, rien affiché), ask (questions dans le front, réponses appliquées en diff à valider).
Un échec de skillforge ne change jamais l'issue d'un run : verdict `indisponible`.
État de skillforge (historique Q/R par skill, journal et cache JEV) : <config>/forge/ (versionné par gitsync).
"""
from __future__ import annotations

import json
import os
import threading
from pathlib import Path

from . import config, paths, secrets


def store() -> Path:
    return paths.config_dir() / "forge"


def _env_key() -> bool:
    """OPENROUTER_API_KEY vient de secrets/infra.env (saisie par l'humain), injectée au moment de l'appel seulement."""
    if os.environ.get("OPENROUTER_API_KEY"):
        return True
    k = secrets.infra_env().get("OPENROUTER_API_KEY")
    if k:
        os.environ["OPENROUTER_API_KEY"] = k
    return bool(k)


def _skill_texts(names: list[str]) -> list[tuple[str, str]]:
    out = []
    for n in names:
        bases = (paths.config_dir() / "skills", paths.tool_dir() / "skills")  # custom d'abord, puis l'outil
        p = next((b / n / "SKILL.md" for b in bases if (b / n / "SKILL.md").exists()), None)
        out.append((n, p.read_text() if p else f"(skill {n} introuvable)"))
    return out


def episode(rec: dict, stream_json: str, *, episode_id: str):
    """Episode skillforge depuis un run (rec = runs/<run>.json, stream_json = déroulé complet déjà masqué)."""
    from skillforge.contract import AgentReport, Episode, SkillRef, Trace
    events = []
    for line in stream_json.split("\n\n--- stderr ---\n", 1)[0].splitlines():
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return Episode(id=episode_id, skills=[SkillRef(id=n, text=t) for n, t in _skill_texts(rec.get("skills") or [])],
                   trace=Trace(format="claude-stream-json", events=events),
                   agent_report=AgentReport(status=rec.get("status"), needs_human=rec.get("needs_human") or None, summary=rec.get("report") or None),
                   meta={k: rec.get(k) for k in ("project", "model", "cost_usd", "num_turns")})


TIMEOUT_S = 120  # au-delà, verdict `indisponible` : un OpenRouter lent ne doit pas retarder le runner


def judge(rec: dict, stream_json: str, *, episode_id: str, mode: str | None = None, timeout_s: float = TIMEOUT_S) -> dict | None:
    """Verdict skillforge (dict JSON) ou None si forge.mode = off. N'élève jamais d'exception, ne dépasse pas `timeout_s`."""
    f = config.host().forge
    mode = mode or f.mode
    if mode == "off":
        return None
    if not _env_key():
        return {"decision": "indisponible", "errors": ["OPENROUTER_API_KEY absent de secrets/infra.env"], "episode_id": episode_id}
    out: dict = {}

    def work():
        try:
            from skillforge import api
            ep = episode(rec, stream_json, episode_id=episode_id)
            v = api.improve(ep, store=store(), draft_with_llm=(mode == "ask" and f.draft_with_llm), tag=episode_id)
            out["v"] = v.model_dump(mode="json")
        except Exception as e:  # jamais bloquant pour infra
            out["v"] = {"decision": "indisponible", "errors": [f"{type(e).__name__}: {e}"[:300]], "episode_id": episode_id}

    t = threading.Thread(target=work, daemon=True)
    t.start()
    t.join(timeout_s)
    if t.is_alive():
        return {"decision": "indisponible", "errors": [f"skillforge : pas de verdict en {timeout_s:.0f} s"], "episode_id": episode_id}
    return out.get("v")


def replay(file: str) -> dict:
    """Rejoue skillforge sur un run passé (runs/<file>.json + .log) et écrit le verdict dedans."""
    p = paths.runs_dir() / file
    rec = json.loads(p.read_text())
    log = p.with_suffix(".log")
    if not log.exists():
        raise FileNotFoundError(f"{log.name} absent (run dry ou ancien)")
    v = judge(rec, log.read_text(), episode_id=p.stem, mode=config.host().forge.mode if config.host().forge.mode != "off" else "shadow")
    rec["forge"] = v
    p.write_text(json.dumps(rec, indent=2, ensure_ascii=False))
    return v


def replay_all() -> list[tuple[str, str]]:
    out = []
    for p in sorted(paths.runs_dir().glob("*.json")):
        if p.with_suffix(".log").exists():
            v = replay(p.name)
            out.append((p.name, v.get("decision") if v else "off"))
    return out


def answer(file: str, text: str, *, apply: bool = False, record: bool = True) -> dict:
    """Réponse de l'auteur à la question d'un run : historique de la skill + portée ; si générale et `apply`,
    la précision est ajoutée à la skill **custom** <config>/skills/<id>/SKILL.md (copie de tool/skills si absente)."""
    from skillforge import api
    from skillforge.contract import Answer
    p = paths.runs_dir() / file
    rec = json.loads(p.read_text())
    v = rec.get("forge") or {}
    if v.get("decision") != "demander" or not v.get("passage"):
        raise ValueError("ce run n'a pas de question skillforge en attente")
    sid = v["passage"]["skill_id"]
    _, skill_text = _skill_texts([sid])[0]
    _env_key()
    a = Answer(skill_id=sid, question=(v.get("question") or {}).get("text") or "", answer=text, episode_id=p.stem)
    if not record:  # aperçu : portée + diff, sans écrire l'historique (le front demande confirmation avant d'appliquer)
        import tempfile
        r = api.answer(a, store=Path(tempfile.mkdtemp()), skill_text=skill_text)
        return r.model_dump(mode="json") | {"preview": True, "custom_exists": (paths.config_dir() / "skills" / sid / "SKILL.md").exists()}
    r = api.answer(a, store=store(), skill_text=skill_text)
    res = r.model_dump(mode="json")
    rec.setdefault("forge_answers", []).append({"answer": text, "scope": r.scope, "applied": False})
    if apply and r.scope == "general":
        dst = paths.config_dir() / "skills" / sid / "SKILL.md"
        dst.parent.mkdir(parents=True, exist_ok=True)
        head = "## Précisions de l'auteur"
        note = ("" if dst.exists() else f"<!-- Copie de tool/skills/{sid} figée par skillforge le {p.stem[:10]} : cette skill custom masque "
                "désormais celle de l'outil ; les évolutions de tool/skills ne s'appliquent plus ici. -->\n")
        body = note + skill_text.rstrip("\n") + ("" if head in skill_text else f"\n\n{head}") + f"\n- {text.strip()}\n"
        dst.write_text(body)
        rec["forge_answers"][-1]["applied"] = str(dst)
        res["applied_to"] = str(dst)
    p.write_text(json.dumps(rec, indent=2, ensure_ascii=False))
    return res
