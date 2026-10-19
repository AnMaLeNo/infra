#!/usr/bin/env bash
# Installe l'outil sur un hôte : dépendances, compte ai-infra, CLI, runner. Idempotent.
# Usage : sudo /srv/infra/tool/install.sh   (repo de config cloné en /srv/infra avec --recursive)
set -euo pipefail

CONFIG_DIR="${INFRA_CONFIG_DIR:-/srv/infra}"
TOOL_DIR="$CONFIG_DIR/tool"
[ "$(id -u)" = 0 ] || { echo "lance avec sudo" >&2; exit 1; }
[ -f "$TOOL_DIR/pyproject.toml" ] || { echo "sous-module tool/ absent : git submodule update --init" >&2; exit 1; }

echo "== paquets"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq git curl ca-certificates gnupg age python3 python3-venv jq unattended-upgrades >/dev/null
if ! command -v docker >/dev/null; then curl -fsSL https://get.docker.com | sh; fi
if ! command -v tailscale >/dev/null; then curl -fsSL https://tailscale.com/install.sh | sh; fi
if ! command -v sops >/dev/null; then
  arch=$(dpkg --print-architecture)
  curl -fsSL -o /usr/local/bin/sops "https://github.com/getsops/sops/releases/download/v3.9.4/sops-v3.9.4.linux.${arch}"
  chmod +x /usr/local/bin/sops
fi
if ! command -v uv >/dev/null; then curl -LsSf https://astral.sh/uv/install.sh | env UV_INSTALL_DIR=/usr/local/bin sh >/dev/null; fi
if ! command -v node >/dev/null; then
  curl -fsSL https://deb.nodesource.com/setup_22.x | bash - >/dev/null && apt-get install -y -qq nodejs >/dev/null
fi
if ! command -v claude >/dev/null; then npm install -g @anthropic-ai/claude-code >/dev/null; fi

echo "== comptes et dossiers"
getent group infra >/dev/null || groupadd infra
if ! id ai-infra >/dev/null 2>&1; then
  useradd -m -s /bin/bash -G docker,infra -c "Agent autonome (infra runner)" ai-infra
fi
usermod -aG docker,infra ai-infra
mkdir -p /srv/apps /srv/data /srv/stack "$CONFIG_DIR/runs"
chgrp -R infra /srv/apps /srv/data /srv/stack "$CONFIG_DIR" 2>/dev/null || true
chmod -R g+rwX /srv/apps /srv/data /srv/stack "$CONFIG_DIR/runs"
chmod g+s /srv/apps /srv/data /srv/stack "$CONFIG_DIR/runs"
# UI (conteneur, uid ai-infra) et runner écrivent la config et la file : groupe infra en écriture
chmod g+rw "$CONFIG_DIR/registry.yaml" "$CONFIG_DIR/host.yaml"
git config --system --add safe.directory '*' 2>/dev/null || true
cat > /etc/sudoers.d/ai-infra <<'SUDO'
Defaults:ai-infra log_input, log_output
ai-infra ALL=(root) NOPASSWD: /usr/bin/systemctl, /usr/bin/tailscale, /usr/bin/apt-get, /usr/local/bin/infra, /usr/sbin/useradd, /usr/sbin/usermod, /usr/sbin/usermod, /usr/bin/passwd -l *, /usr/bin/tee /etc/sudoers.d/ai-*
SUDO
chmod 0440 /etc/sudoers.d/ai-infra

echo "== CLI"
uv tool install --force --python 3.11 "$TOOL_DIR" >/dev/null 2>&1 || uv tool install --force "$TOOL_DIR" >/dev/null
ln -sf "$(uv tool dir)/infra/bin/infra" /usr/local/bin/infra
infra --version

echo "== runner (systemd)"
sed "s#@CONFIG_DIR@#$CONFIG_DIR#g" "$TOOL_DIR/deploy/infra-runner.service" > /etc/systemd/system/infra-runner.service
systemctl daemon-reload
systemctl enable infra-runner >/dev/null

echo
echo "Note : sshd n'est pas touché (PasswordAuthentication, clés) — à faire à la main quand tout est stable."
echo "OK. Suite : skills/bootstrap-host/SKILL.md (clé age → $CONFIG_DIR/.age-key, tailscale up, infra stack up traefik…)"
