# Server details

The server details page shows the details of a single Nginx server block.

[//]: # (<img src="/images/server-details.png">)

The following information is available:

- PHP Version configured with the block
- SSL Certification creation date
- Docroot path of the server
- Database access details
- FTP Users with access to the server
- Git repository if configured


## Initial configuration

The initial configuration of the server is done using the admin UI.
The following options are available:
- Create SSL certificate using Certbot (Let's Encrypt)
- Create MySQL database and user for the server
- Create FTP user for the server

## Choose PHP version

There are multiple PHP versions available. The default version is PHP 8.1.
Configured PHP versions can be edited in the application's `.env` file.

## Server settings

Users can edit the document root, and source repository of the server.
More info about deployments in the Deployment section.

## Config template

Config templates are used to generate the Nginx configuration for the
server. The default template is a simple PHP app. Users can create
servers using the following application templates:

Template files are located in `resources/views/sample/nginx-*.blade.php`
files.

**Default PHP Project** (`default`)

Basic Nignx setup to support PHP based projects. Includes PHP-FPM
configuration and a default landing page.

**WordPress Configuration** (`wordpress`)

Nginx configuration for WordPress based projects. Includes PHP-FPM
configuration and WordPress specific Nginx options.

**Drupal Recommended** (`drupal`)

Drupal specific Nginx configuration. Includes PHP-FPM configuration
and Drupal specific Nginx options.


**Laravel Configuration** (`laravel`)

Nginx configuration for Laravel based projects. Includes PHP-FPM
configuration and Laravel specific Nginx options.


**Static HTML** (`html`)

Config to serve static HTML pages with Nginx. Includes PHP-FPM
configuration and a default landing page.
