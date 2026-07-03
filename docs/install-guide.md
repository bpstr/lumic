---
outline: deep
---

# Install Lumic on Ubuntu 20.04

Run the following commands to install Lumic on Ubuntu 20.04.

```bash
bash <(curl -s https://raw.githubusercontent.com/bpstr/lumic/main/lumic.sh)
```

This script automates the setup and configuration of a Lumic server manager on a fresh Ubuntu 20.04 instance. It streamlines the installation of various components, including NGINX, MySQL, PHP, Composer, and Certbot, among others.

_After setup is complete, the Lumic PHP application will be accessible at the server's public IP address. Password to log in can be found in the `var/www/html/.env` file._

## Production Scope

Run `lumic.sh` only on a fresh VPS dedicated to Lumic. The script installs packages, replaces Nginx configuration, creates system users, changes `/var/www` permissions, configures MariaDB, enables UFW, installs Certbot, and writes cron entries. These actions are not safe for a local workstation or a shared production host.

## Configuration Options:

These variables can be set before running the script to customize the installation according to specific needs. If not set, the script will use the default values provided.

`APP_NAME`: The name of the application. Default is "Lumic".

`APP_HOST`: The host of the application. Default is the server's public IP.

`APP_MAIL`: Webmaster email. Default is 'webmaster@example.com'.

`ROOT_USER_NAME`: The name of the root user being created. Default is "lumic".

`ROOT_USER_PASS`: The password for the root user. Default is a generated secure password.

`MYSQL_ROOT_USER`: The MySQL root username. Default is "lumic".

`MYSQL_ROOT_PASS`: The password for the MySQL root user. Default is a generated secure password.

`WEBMASTER_EMAIL`: Email passed to Certbot. Default is `webmaster@example.com`.

`AVAILABLE_PHP_VERSIONS`: Comma-separated PHP versions users may select in Lumic.

`NGINX_ROOT_PATH`: Nginx root path. Default is `/etc/nginx`.

`NGINX_LOG_PATH`: Nginx log path. Default is `/var/log/nginx/`.

`DOCROOT_PATH`: Site document-root base. Default is `/var/www`.

`GITROOT_PATH`: Git checkout base. Default is `/var/git`.

`QUEUE_CONNECTION`: Queue driver. Production installs use `database`.








## Components Installed

### System Configuration

- Fetches the public IP of the server.
- Sets up logging to /var/log/installscript.log.

### User Management

- Creates a root user named lumic (configurable) with a secure, randomly generated password.
Web Server (NGINX):

### Installs and configures NGINX
- Sets up a default landing page indicating the server setup status.

### Database (MySQL)

- Installs MariaDB server.
- Secures the MySQL installation by removing anonymous users, disallowing remote root login, and removing the test database.
- Sets a secure password for the MySQL root user.

### PHP

- Installs PHP 8.1 along with various extensions.
- Configures PHP-FPM.

### Development tools

- Downloads and installs Composer globally.

### Lumic PHP Application

- Clones the Lumic PHP application from its GitHub repository.
- Sets appropriate permissions and installs necessary dependencies.
- Configures the application environment.

### Firewall (UFW)

- Installs and configures UFW (Uncomplicated Firewall).
- Allows SSH, HTTP, and HTTPS traffic.

### SSL (Certbot)

- Installs Certbot using Snap for SSL certificate management.

### Cron Jobs

- Sets up cron jobs for Certbot renewal and Lumic PHP tasks.

## Post-Installation
Once the script completes, the Lumic server should be fully set up and operational. The default web page will display the server setup status, and upon completion, the Lumic PHP application will be accessible.

### Important Notes

- This script assumes it's being run on a fresh Linode Ubuntu 20.04 instance.
- Always test the script in a staging environment before deploying to production.
- Regularly review and update the script to accommodate changes in software packages, repositories, and best practices.
- The script currently assumes Ubuntu-style service names and paths. Review it before using a newer Ubuntu release.
- Generated passwords are written to `/var/www/html/.env`; restrict shell and file access to that file.
- `nginx:restart`, `ssl:certificate`, `db:create`, `git:deploy`, and `ftp:create-user` are production-only commands because they touch host services or system paths.

## Troubleshooting

### Nginx

Run `sudo nginx -t` before restarting. Check `/etc/nginx/nginx.conf`, `/etc/nginx/sites-enabled/home.conf`, generated server blocks, and `/var/log/nginx/error.log`.

### Certbot

Confirm DNS points to the VPS and UFW allows HTTP/HTTPS. Retry after fixing DNS or firewall issues.

### MySQL or MariaDB

Check `sudo systemctl status mariadb`, then verify `MYSQL_ROOT_USER` and `MYSQL_ROOT_PASS` in `/var/www/html/.env`.

### Permissions

The web user must write to `storage`, `database`, generated Nginx blocks, docroots, and deploy logs. A common recovery command is:

```bash
sudo chown -R www-data:www-data /var/www/html
sudo chmod -R u+rwX /var/www/html/storage /var/www/html/database
```

### Cron and Queues

Inspect `/etc/cron.d/lumic.crontab` and the active crontab. The install script configures schedule and queue workers to run once per minute.

### Security

The script generates secure, random passwords for various components and sets appropriate permissions for files and directories. However, always ensure to follow best security practices and review configurations as needed.



