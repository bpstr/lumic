# Lumic Server Management

Lumic is a server management system for web servers. It is designed to be a simple and easy to use system for managing your web servers on a single VPS. Lumic is built on top of the Lumen PHP framework.

## Installation

Install stackscript on Linode. This will install all the necessary packages and dependencies to run Lumic. 

Alternatively run the following command on any Ubuntu 20.04 server to install Lumic.
```bash
bash <(curl -s https://raw.githubusercontent.com/bpstr/lumic/main/install.sh)
```

## Features

- [x] Create Nginx server blocks and configuration
- [ ] Manage PHP versions and extensions
- [x] Create SSL certificates with Let's Encrypt
- [x] Create MySQL databases and users
- [ ] Create and manage FTP users
- [ ] Manage deployments from Git repositories
- [ ] Manage cron jobs
- [ ] Manage domain aliases (server names)
- [ ] List files and folders

## Lumen PHP Framework

Laravel Lumen is a stunningly fast PHP micro-framework for building web applications with expressive, elegant syntax. We believe development must be an enjoyable, creative experience to be truly fulfilling. Lumen attempts to take the pain out of development by easing common tasks used in the majority of web projects, such as routing, database abstraction, queueing, and caching.

### Official Documentation

Documentation for the framework can be found on the [Lumen website](https://lumen.laravel.com/docs).

### Security Vulnerabilities

If you discover a security vulnerability within Lumen, please send an e-mail to Taylor Otwell at taylor@laravel.com. All security vulnerabilities will be promptly addressed.

### License

The Lumen framework is open-sourced software licensed under the [MIT license](https://opensource.org/licenses/MIT).
