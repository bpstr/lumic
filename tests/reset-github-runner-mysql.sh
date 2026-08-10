#!/usr/bin/env sh
set -eu

if [ "${RUNNER_ENVIRONMENT:-}" != "github-hosted" ]; then
  echo 'refusing to reset MySQL outside an ephemeral GitHub-hosted runner' >&2
  exit 1
fi

sudo systemctl stop mysql.service || true
sudo env DEBIAN_FRONTEND=noninteractive apt-get purge -y \
  mysql-server \
  mysql-server-8.0 \
  mysql-server-core-8.0 \
  mysql-client \
  mysql-client-8.0 \
  mysql-client-core-8.0
sudo rm -rf -- /var/lib/mysql
