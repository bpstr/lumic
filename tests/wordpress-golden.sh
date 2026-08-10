#!/usr/bin/env sh
set -eux

: "${LUMIC_TEST_BINARY:?set LUMIC_TEST_BINARY to the built lumic CLI}"
case "$LUMIC_TEST_BINARY" in
  /*) LUMIC_BIN="$LUMIC_TEST_BINARY" ;;
  *) LUMIC_BIN="$(pwd)/$LUMIC_TEST_BINARY" ;;
esac

TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT INT TERM
chmod 755 "$TEST_ROOT"
export LUMIC_STATE_DIR="$TEST_ROOT/state"
export LUMIC_APPS_ROOT="$TEST_ROOT/apps"

"$LUMIC_BIN" recipe catalog > "$TEST_ROOT/catalog.json"
grep -q '"id": "wordpress"' "$TEST_ROOT/catalog.json"
grep -q 'wordpress-6.8.2.tar.gz' "$TEST_ROOT/catalog.json"
grep -q 'wp-cli-2.12.0.phar' "$TEST_ROOT/catalog.json"

"$LUMIC_BIN" recipe plan wordpress blog blog.lumic.test \
  --env WORDPRESS_SITE_TITLE='Lumic WordPress' \
  --env WORDPRESS_ADMIN_USER=lumic_admin \
  --env WORDPRESS_ADMIN_EMAIL=admin@lumic.test > "$TEST_ROOT/plan.json"
grep -q 'managed_service.install' "$TEST_ROOT/plan.json"

"$LUMIC_BIN" recipe install wordpress blog blog.lumic.test \
  --env WORDPRESS_SITE_TITLE='Lumic WordPress' \
  --env WORDPRESS_ADMIN_USER=lumic_admin \
  --env WORDPRESS_ADMIN_EMAIL=admin@lumic.test > "$TEST_ROOT/install.json"
grep -q '"changed": true' "$TEST_ROOT/install.json"
grep -q '"status": "installed"' "$TEST_ROOT/install.json"
test -f "$LUMIC_APPS_ROOT/blog/current/wp-settings.php"
test -f "$LUMIC_APPS_ROOT/blog/current/wp-config.php"
php8.3 "$LUMIC_STATE_DIR/artifacts/wp-cli-2.12.0.phar" \
  --path="$LUMIC_APPS_ROOT/blog/current" --allow-root core is-installed
curl --fail --silent --show-error --header 'Host: blog.lumic.test' \
  http://127.0.0.1/wp-login.php > /dev/null

cp "$LUMIC_STATE_DIR/resources.json" "$TEST_ROOT/resources-first.json"
"$LUMIC_BIN" recipe install wordpress blog blog.lumic.test > "$TEST_ROOT/second.json"
grep -q '"changed": false' "$TEST_ROOT/second.json"
cmp "$TEST_ROOT/resources-first.json" "$LUMIC_STATE_DIR/resources.json"
test "$(grep -c '"id": "wordpress.6.8.2"' "$LUMIC_STATE_DIR/resources.json")" -eq 1
test "$(grep -c '"id": "wp-cli.2.12.0"' "$LUMIC_STATE_DIR/resources.json")" -eq 1

"$LUMIC_BIN" recipe uninstall blog > "$TEST_ROOT/uninstall.json"
grep -q 'moved to Lumic trash' "$TEST_ROOT/uninstall.json"
test ! -e "$LUMIC_APPS_ROOT/blog"
test ! -e /etc/nginx/sites-available/lumic-blog.conf
test ! -e /etc/nginx/sites-enabled/lumic-blog.conf
grep -q 'database.blog-mysql-blog_wp' "$LUMIC_STATE_DIR/resources.json"
test -f "$LUMIC_STATE_DIR/artifacts/wordpress-6.8.2.tar.gz"
test -f "$LUMIC_STATE_DIR/artifacts/wp-cli-2.12.0.phar"
