<p align="center"><a href="https://laravel.com" target="_blank">
    <img src="https://raw.githubusercontent.com/bpstr/lumic/main/public/lumic.svg#gh-light-mode-only" width="420" alt="Lumic Logo">
</a></p>

<p align="center">
<a href="https://www.linux.org/" title="Go to Linux homepage"><img src="https://img.shields.io/badge/OS-Linux-blue?logo=linux&logoColor=white" alt="OS - Linux"></a>
<a href="https://lumic.cc/"><img src="https://img.shields.io/static/v1?label=License&message=MIT&color=2ea44f" alt="License - MIT"></a>
</p>


# Lumic Server Management

Lumic is a server management system for web servers. It is designed to be a simple and easy to use system for managing your web servers on a single VPS. Lumic is built on top of the Lumen PHP framework.

## Installation

Install stackscript on Linode. This will install all the necessary packages and dependencies to run Lumic. 

Alternatively run the following command on any Ubuntu 20.04 server to install Lumic.
```bash
bash <(curl -s https://raw.githubusercontent.com/bpstr/lumic/main/lumic.sh)
```

## Features

- [x] Create Nginx server blocks and configuration
- [x] Manage PHP versions and extensions
- [x] Create SSL certificates with Let's Encrypt
- [x] Create MySQL databases and users
- [x] Create and manage SFTP users
- [x] Manage deployments from Git repositories
- [x] Manage cron jobs
- [x] Manage domain aliases (server names)
- [x] List files and folders

## Production Requirements

Lumic is intended for a single fresh Ubuntu VPS where it is allowed to manage host services. Do not run production commands on a development laptop or shared host unless you have isolated the environment.

Required host services and paths:

- Ubuntu 20.04 compatible package repositories
- Nginx with configuration under `/etc/nginx`
- PHP-FPM sockets under `/var/run/php/php{version}-fpm.sock`
- MariaDB or MySQL available to the configured root/admin user
- Certbot for Let's Encrypt certificates
- Writable project roots at `/var/www` and `/var/git`
- Writable application storage at `/var/www/html/storage`

Required environment variables:

- `APP_NAME`, `APP_ENV`, `APP_KEY`, `APP_URL`, `APP_IP`, `APP_TIMEZONE`
- `WEBMASTER_EMAIL`
- `ROOT_USER_NAME`, `ROOT_USER_PASS`
- `MYSQL_ROOT_USER`, `MYSQL_ROOT_PASS`
- `AVAILABLE_PHP_VERSIONS`
- `NGINX_ROOT_PATH`, `NGINX_LOG_PATH`, `DOCROOT_PATH`, `GITROOT_PATH`
- `DB_CONNECTION`, `CACHE_DRIVER`, `QUEUE_CONNECTION`

Safe locally: validation tests, model tests, Blade rendering, and code that fakes Artisan/process calls.

Production VPS only: `lumic.sh`, `dir:prepare`, `db:create`, `ssl:certificate`, `nginx:test`, `nginx:restart`, `git:deploy`, `ftp:create-user`, and any command that writes to `/var/www`, `/var/git`, `/etc/nginx`, MySQL, Certbot, system users, or system cron.

Install script recovery notes:

- If Nginx fails, run `nginx -t`, inspect `/etc/nginx/nginx.conf` and `/etc/nginx/sites-enabled/home.conf`, then restart with `sudo service nginx restart`.
- If Certbot fails, verify DNS points to the VPS and ports 80/443 are open, then rerun the certificate command for the affected server.
- If MySQL setup fails, verify `MYSQL_ROOT_USER` and `MYSQL_ROOT_PASS` in `/var/www/html/.env`, then check MariaDB status with `sudo systemctl status mariadb`.
- If permissions fail, restore application ownership with `sudo chown -R www-data:www-data /var/www/html` and ensure `storage` and `database` are writable by the web user.
- The installer writes `/etc/cron.d/lumic.crontab` and installs it with `crontab`; inspect both if queue or scheduled tasks stop running.

## Lumen PHP Framework

Laravel Lumen is a stunningly fast PHP micro-framework for building web applications with expressive, elegant syntax. We believe development must be an enjoyable, creative experience to be truly fulfilling. Lumen attempts to take the pain out of development by easing common tasks used in the majority of web projects, such as routing, database abstraction, queueing, and caching.

### Official Documentation

Documentation for the framework can be found on the [Lumen website](https://lumen.laravel.com/docs).

### Security Vulnerabilities

If you discover a security vulnerability within Lumen, please send an e-mail to Taylor Otwell at taylor@laravel.com. All security vulnerabilities will be promptly addressed.

### License

The Lumen framework is open-sourced software licensed under the [MIT license](https://opensource.org/licenses/MIT).


### Troubleshooting

```bash
sudo chown -R www-data:www-data /var/www
```

#### Documentation
