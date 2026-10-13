# Image de l'UI/API (stack/ui). Le runner (claude -p) tourne sur l'hôte, pas ici.
FROM python:3.12-slim
RUN apt-get update && apt-get install -y --no-install-recommends git curl ca-certificates && rm -rf /var/lib/apt/lists/* \
 && curl -fsSL -o /usr/local/bin/sops "https://github.com/getsops/sops/releases/download/v3.9.4/sops-v3.9.4.linux.$(dpkg --print-architecture)" \
 && chmod +x /usr/local/bin/sops \
 && curl -fsSL https://download.docker.com/linux/static/stable/$(uname -m)/docker-27.5.1.tgz | tar xz -C /tmp \
 && mv /tmp/docker/docker /usr/local/bin/docker && rm -rf /tmp/docker
WORKDIR /app
COPY pyproject.toml README.md ./
COPY infra ./infra
RUN pip install --no-cache-dir .
ENV INFRA_CONFIG_DIR=/srv/infra
EXPOSE 8000
CMD ["infra", "serve", "--host", "0.0.0.0", "--port", "8000"]
