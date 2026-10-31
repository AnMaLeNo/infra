#!/usr/bin/env bash
# Installe l'outil sur un hôte : dépendances, compte ai-infra, CLI, runner. Idempotent.
# Usage : sudo /srv/infra/tool/install.sh   (repo de config cloné en /srv/infra avec --recursive)
set -euo pipefail

CONFIG_DIR="${INFRA_CONFIG_DIR:-/srv/infra}"
TOOL_DIR="$CONFIG_DIR/tool"
[ "$(id -u)" = 0 ] || { echo "lance avec sudo" >&2; exit 1; }
[ -f "$TOOL_DIR/Cargo.toml" ] || { echo "sous-module tool/ absent : git submodule update --init" >&2; exit 1; }

echo "== paquets"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq git curl ca-certificates gnupg age jq unattended-upgrades >/dev/null
if ! command -v docker >/dev/null; then curl -fsSL https://get.docker.com | sh; fi
if ! command -v tailscale >/dev/null; then curl -fsSL https://tailscale.com/install.sh | sh; fi
if ! command -v sops >/dev/null; then
  arch=$(dpkg --print-architecture)
  curl -fsSL -o /usr/local/bin/sops "https://github.com/getsops/sops/releases/download/v3.9.4/sops-v3.9.4.linux.${arch}"
  chmod +x /usr/local/bin/sops
fi
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
mkdir -p /srv/apps /srv/data /srv/stack /srv/shared "$CONFIG_DIR/runs" "$CONFIG_DIR/shared"
chgrp -R infra /srv/apps /srv/data /srv/stack /srv/shared "$CONFIG_DIR" 2>/dev/null || true
chmod -R g+rwX /srv/apps /srv/data /srv/stack /srv/shared "$CONFIG_DIR/runs" "$CONFIG_DIR/shared"
chmod g+s /srv/apps /srv/data /srv/stack /srv/shared "$CONFIG_DIR/runs" "$CONFIG_DIR/shared"
# UI (conteneur, uid ai-infra) et runner écrivent la config et la file : groupe infra en écriture
# (shared.yaml est créé ici s'il manque : l'UI ne peut pas créer de fichier dans $CONFIG_DIR, qui reste root)
[ -f "$CONFIG_DIR/shared.yaml" ] || printf 'services: {}\n' > "$CONFIG_DIR/shared.yaml"
chgrp infra "$CONFIG_DIR/registry.yaml" "$CONFIG_DIR/host.yaml" "$CONFIG_DIR/shared.yaml"
chmod g+rw "$CONFIG_DIR/registry.yaml" "$CONFIG_DIR/host.yaml" "$CONFIG_DIR/shared.yaml"
# le runner journalise les migrations dans docs/migrations.md
mkdir -p "$CONFIG_DIR/docs"
chgrp -R infra "$CONFIG_DIR/docs" && chmod -R g+rwX "$CONFIG_DIR/docs" && chmod g+s "$CONFIG_DIR/docs"
git config --system --add safe.directory '*' 2>/dev/null || true
# le runner (ai-infra) commite et pousse la config après chaque run : dépôt partagé avec le groupe infra
if [ -d "$CONFIG_DIR/.git" ]; then
  chgrp -R infra "$CONFIG_DIR/.git" && chmod -R g+rwX "$CONFIG_DIR/.git" && find "$CONFIG_DIR/.git" -type d -exec chmod g+s {} +
  git -C "$CONFIG_DIR" config core.sharedRepository group
fi
cat > /etc/sudoers.d/ai-infra <<'SUDO'
Defaults:ai-infra log_input, log_output
ai-infra ALL=(root) NOPASSWD: /usr/bin/systemctl, /usr/bin/tailscale, /usr/bin/apt-get, /usr/local/bin/infra, /usr/sbin/useradd, /usr/sbin/usermod, /usr/sbin/usermod, /usr/bin/passwd -l *, /usr/bin/tee /etc/sudoers.d/ai-*
SUDO
chmod 0440 /etc/sudoers.d/ai-infra

echo "== CLI"
# Binaire unique (Rust, front embarqué) : celui du dépôt s'il a été compilé sur place, sinon celui attaché à la release
# GitHub du tag sur lequel est le sous-module tool/ (CI : job `binaries`).
arch=$(uname -m)
if [ -x "$TOOL_DIR/target/release/infra" ]; then
  install -m 0755 "$TOOL_DIR/target/release/infra" /usr/local/bin/infra
else
  tag=$(git -C "$TOOL_DIR" describe --tags --exact-match 2>/dev/null || true)
  [ -n "$tag" ] || { echo "tool/ n'est pas sur un tag vX.Y.Z et n'a pas de binaire compilé (cargo build --release)" >&2; exit 1; }
  curl -fsSL -o /usr/local/bin/infra.new "https://github.com/AnMaLeNo/infra/releases/download/$tag/infra-$arch"
  chmod 0755 /usr/local/bin/infra.new && mv /usr/local/bin/infra.new /usr/local/bin/infra
fi
rm -rf /usr/local/lib/infra-uv   # ancienne installation Python (uv tool)
infra --version

echo "== runner (systemd)"
sed "s#@CONFIG_DIR@#$CONFIG_DIR#g" "$TOOL_DIR/deploy/infra-runner.service" > /etc/systemd/system/infra-runner.service
systemctl daemon-reload
systemctl enable infra-runner >/dev/null

echo "== éditeur (VS Code de Microsoft en web, code serve-web, privé via Traefik : editor.<private_zone>)"
case "$(uname -m)" in aarch64|arm64) VSC_OS=cli-alpine-arm64 ;; *) VSC_OS=cli-alpine-x64 ;; esac
if ! command -v vscode-cli >/dev/null; then
  mkdir -p /usr/local/lib/vscode-cli
  curl -fsSL "https://code.visualstudio.com/sha/download?build=stable&os=$VSC_OS" | tar -xz -C /usr/local/lib/vscode-cli
  ln -sfn /usr/local/lib/vscode-cli/code /usr/local/bin/vscode-cli
fi
sed "s#@CONFIG_DIR@#$CONFIG_DIR#g" "$TOOL_DIR/deploy/infra-editor.service" > /etc/systemd/system/infra-editor.service
systemctl daemon-reload
systemctl enable --now infra-editor >/dev/null

echo
echo "Note : sshd n'est pas touché (PasswordAuthentication, clés) — à faire à la main quand tout est stable."
echo "OK. Suite : skills/bootstrap-host/SKILL.md (clé age → $CONFIG_DIR/.age-key, tailscale up, infra stack up traefik…)"
