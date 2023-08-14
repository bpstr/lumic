---
outline: deep
---

# Install Lumic on Ubuntu 20.04

Run the following commands to install Lumic on Ubuntu 20.04.

```bash
bash <(curl -s https://raw.githubusercontent.com/bpstr/lumic/main/install.sh)
```

This script automates the setup and configuration of a Lumic server manager on a fresh Ubuntu 20.04 instance. It streamlines the installation of various components, including NGINX, MySQL, PHP, Composer, and Certbot, among others.

_After setup is complete, the Lumic PHP application will be accessible at the server's public IP address. Password to log in can be found in the `var/www/html/.env` file._

## Configuration Options:

These variables can be set before running the script to customize the installation according to specific needs. If not set, the script will use the default values provided.

`APP_NAME`: The name of the application. Default is "Lumic".

`APP_HOST`: The host of the application. Default is the server's public IP.

`APP_MAIL`: Webmaster email. Default is 'webmaster@example.com'.

`ROOT_USER_NAME`: The name of the root user being created. Default is "lumic".

`ROOT_USER_PASS`: The password for the root user. Default is a generated secure password.

`MYSQL_ROOT_USER`: The MySQL root username. Default is "lumic".

`MYSQL_ROOT_PASS`: The password for the MySQL root user. Default is a generated secure password.








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

### Security

The script generates secure, random passwords for various components and sets appropriate permissions for files and directories. However, always ensure to follow best security practices and review configurations as needed.




