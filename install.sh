#!/bin/bash

IP=$(curl -s https://checkip.amazonaws.com)

APP_NAME=${APP_NAME:-Lumic}
APP_HASH=$(openssl rand -base64 32 | sha256sum | base64 | head -c 32 | tr '[:upper:]' '[:lower:]')
APP_HOST=${APP_HOST:-$IP}
APP_MAIL=${APP_MAIL:-'webmaster@example.com'}

ROOT_USER_NAME=${ROOT_USER_NAME:-lumic}
ROOT_USER_PASS=${ROOT_USER_PASS:-$(openssl rand -base64 32|sha256sum|base64|head -c 32| tr '[:upper:]' '[:lower:]')}
MYSQL_ROOT_USER=${MYSQL_ROOT_USER:-lumic}
MYSQL_ROOT_PASS=${MYSQL_ROOT_PASS:-$(openssl rand -base64 32|sha256sum|base64|head -c 32| tr '[:upper:]' '[:lower:]')}

exec > >(tee /dev/ttyS0 /var/log/installscript.log) 2>&1
DEBIAN_FRONTEND=noninteractive apt-get update -qq >/dev/null
###########################################################
# Create root user
###########################################################
echo "Creating root user..." >> /var/www/html/status.txt
sudo pam-auth-update --package
sudo mount -o remount,rw /
sudo chmod 640 /etc/shadow
sudo useradd -m -s /bin/bash lumic
echo "${ROOT_USER_NAME}:${ROOT_USER_PASS}"|sudo chpasswd
###########################################################
# Install NGINX
###########################################################
echo "Installing nginx..." >> /var/www/html/status.txt
apt-get install -y nginx
cat <<'END' >/var/www/html/index.html
<!doctype html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Setting up Lumic server</title>
    <link href="https://cdn.jsdelivr.net/npm/bootstrap@5.3.1/dist/css/bootstrap.min.css" rel="stylesheet" integrity="sha384-4bw+/aepP/YC94hEpVNVgiZdgIC5+VKNBQNGCHeKRQN+PtmoHDEXuppvnDJzQIu9" crossorigin="anonymous">
</head>
<body class="bg-dark">
    <div class="h-100 w-100 d-flex flex-column justify-content-center">
        <div class="mt-5 pt-5 col-md-4 mx-auto text-center">
            <h1 class="display-2 fw-bold text-light">Setting up...</h1>
            <div class="card rounded-0 my-4 p-3 bg-black">
                <p class="mb-3 text-sm text-light">Current status of install script (this page can be closed):</p>
                <iframe src="status.txt" id="console" class="w-full border-2 border-primary bg-white rounded-md overflow-hidden"></iframe>
            </div>
        </div>
    </div>
    <script src="https://cdn.jsdelivr.net/npm/bootstrap@5.3.1/dist/js/bootstrap.bundle.min.js" integrity="sha384-HwwvtgBNo3bZJJLYd8oVXjrBZt8cqVSpeBNS5n7C8IVInixGAoxmnlMuBnhbgrkm" crossorigin="anonymous"></script>
</body>
</html>
END
chown www-data:www-data /var/www/html/index.html
chmod 644 /var/www/html/index.html
sudo systemctl enable nginx.service
###########################################################
# MySQL
###########################################################
echo "Setting up MySQL server..." >> /var/www/html/status.txt
apt install -y mariadb-server expect
function mysql_secure_install {
    # $1 - required - Root password for the MySQL database
    [ ! -n "$1" ] && {
        printf "mysql_secure_install() requires the MySQL database root password as its only argument\n"
        return 1;
    }
    local -r db_root_password="$1"
    local -r secure_mysql=$(
expect -c "
set timeout 10
spawn mysql_secure_installation
expect \"Enter current password for root (enter for none):\"
send \"$db_root_password\r\"
expect \"Change the root password?\"
send \"n\r\"
expect \"Remove anonymous users?\"
send \"y\r\"
expect \"Disallow root login remotely?\"
send \"y\r\"
expect \"Remove test database and access to it?\"
send \"y\r\"
expect \"Reload privilege tables now?\"
send \"y\r\"
expect eof
")
    printf "$secure_mysql\n"
}
# Set DB root password
echo "mysql-server mysql-server/root_password password ${MYSQL_ROOT_PASS}" | debconf-set-selections
echo "mysql-server mysql-server/root_password_again password ${MYSQL_ROOT_PASS}" | debconf-set-selections
mysql_secure_install "$MYSQL_ROOT_PASS"
###########################################################
# Install PHP
###########################################################
echo "Setting up PHP 8.1..." >> /var/www/html/status.txt
apt-get install -y software-properties-common
add-apt-repository ppa:ondrej/php -y
apt-get update
apt-get install -y zip unzip php8.1-mbstring php8.1-zip php8.1-gd php8.1-cli php8.1-curl php8.1-intl php8.1-imap php8.1-xml php8.1-xsl php8.1-tokenizer php8.1-sqlite3 php8.1-pgsql php8.1-opcache php8.1-simplexml php8.1-fpm php8.1-bcmath php8.1-ctype php8.1-pdo php8.1-mysql php8.1-xml
service php8.1-fpm restart
###########################################################
# Composer
###########################################################
echo "Setting up Composer..." >> /var/www/html/status.txt
php -r "copy('https://getcomposer.org/installer', 'composer-setup.php');"
php composer-setup.php --no-interaction
php -r "unlink('composer-setup.php');"
mv composer.phar /usr/local/bin/composer
COMPOSER_ALLOW_SUPERUSER=1 composer config --global repo.packagist composer https://packagist.org --no-interaction
###########################################################
# Lumic PHP
###########################################################
echo "Setting up Lumic PHP..." >> /var/www/html/status.txt
mkdir -p /var/git && cd /var/git \
 && git clone 'https://github.com/bpstr/lumic/'
rsync -av --exclude-from=/var/git/lumic/resources/lists/default-excluded.lst /var/git/lumic/ /var/www/html/
chown -R :www-data /var/www/html
chmod -R 775 /var/www/html/storage
chmod -R 775 /var/www/html/database/database.sqlite
# Fix permissions
chown -Rf www-data:www-data /var/www/html
find /var/www/html/ -type d -exec chmod 755 {} \;
find /var/www/html/ -type f -exec chmod 644 {} \;
sudo chmod o+w /var/www/html/storage/ -R
# Install dependencies
cd /var/www/html && sudo -u www-data composer install --no-interaction
cat <<END >/var/www/html/.env
APP_NAME=${APP_NAME}
APP_ENV=production
APP_KEY=${APP_HASH}
APP_DEBUG=false
APP_URL=http://${APP_HOST}
APP_IP=${IP}
APP_TIMEZONE=UTC
WEBMASTER_EMAIL=${APP_MAIL}
ROOT_USER_NAME=${ROOT_USER_NAME}
ROOT_USER_PASS=${ROOT_USER_PASS}
MYSQL_ROOT_USER=${MYSQL_ROOT_USER}
MYSQL_ROOT_PASS=${MYSQL_ROOT_PASS}
AVAILABLE_PHP_VERSIONS=7.4,8.0,8.1,8.2,8.3
NGINX_ROOT_PATH='/etc/nginx'
NGINX_LOG_PATH='/var/log/nginx/'
DOCROOT_PATH='/var/www'
LOG_CHANNEL=stack
LOG_SLACK_WEBHOOK_URL=
DB_CONNECTION=sqlite
CACHE_DRIVER=file
QUEUE_CONNECTION=database
END
cd /var/www/html && php artisan key:generate
# Fix permissions
sudo chmod o+w /var/www/html/storage/ -R
# Install lumic
php /var/www/html/artisan migrate:fresh --force --seed --no-interaction
###########################################################
# Configure NGINX
###########################################################
echo "Configuring Nginx..." >> /var/www/html/status.txt
PHP_VERSION=$(php -r "echo PHP_MAJOR_VERSION.'.'.PHP_MINOR_VERSION;")
cat << END > /etc/nginx/nginx.conf
# Generic startup file.
user www-data;
#usually equal to number of CPUs you have. run command "grep processor /proc/cpuinfo | wc -l" to find it
worker_processes  auto;
worker_cpu_affinity auto;
error_log  /var/log/nginx/error.log;
pid        /var/run/nginx.pid;
# Keeps the logs free of messages about not being able to bind().
#daemon     off;
events {
worker_connections  1024;
}
http {
#   rewrite_log on;
include mime.types;
default_type       application/octet-stream;
access_log         /var/log/nginx/access.log;
sendfile           on;
#   tcp_nopush         on;
keepalive_timeout  64;
#   tcp_nodelay        on;
#   gzip               on;
        #php max upload limit cannot be larger than this
client_max_body_size 13m;
index              index.php index.html index.htm;
# Upstream to abstract backend connection(s) for PHP.
upstream php {
        #this should match value of "listen" directive in php-fpm pool
        server unix:/run/php/php8.1-fpm.sock;
        server 127.0.0.1:9000;
}

include /etc/nginx/sites-enabled/*;
}
END
unlink /etc/nginx/sites-enabled/default
unlink /etc/nginx/sites-available/default
cat << 'EOF' > /etc/nginx/sites-enabled/home.conf
server {
        listen 80 default_server;
        listen [::]:80 default_server;

        server_name _;

        root /var/www/html/public;
        add_header X-Frame-Options "SAMEORIGIN";
        add_header X-XSS-Protection "1; mode=block";
        add_header X-Content-Type-Options "nosniff";
        index index.html index.htm index.php;
        charset utf-8;
        location / {
                try_files $uri $uri/ /index.php?$query_string;
        }

        location ^~ /livewire/ {
            try_files  / =404;
        }

        # Prevent Direct Access To Protected Files
        location ~ \.(env|log) {
                deny all;
        }
        # Prevent Direct Access To Protected Folders
        location ~ ^/(^app$|bootstrap|config|database|overrides|resources|routes|tests|artisan) {
                deny all;
        }
        # Prevent Direct Access To modules/vendor Folders Except Assets
        location ~ ^/(modules|vendor|livewire)/(.*)\.((?!ico|gif|jpg|jpeg|png|js\b|css|less|sass|font|woff|woff2|eot|ttf|svg).)*$ {
                deny all;
        }
        error_page 404 /index.php;
        # Pass PHP Scripts To FastCGI Server
        location ~ \.php$ {
                fastcgi_split_path_info ^(.+\.php)(/.+)$;
                fastcgi_pass php;
                fastcgi_index index.php;
                fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
                include fastcgi_params;
        }
        location ~ /\.(?!well-known).* {
                deny all;
        }
}
EOF

# Remove installation screen
rm -f /var/www/html/index.html
sudo service nginx restart
###########################################################
# Firewall
###########################################################
echo "Installing firewall..." >> /var/www/html/status.txt
apt-get install ufw -y
ufw limit ssh
ufw allow http
ufw allow https
ufw --force enable
###########################################################
# Certbot
###########################################################
echo "Setting up certbot..." >> /var/www/html/status.txt
apt-get install snapd -y
snap install core
snap refresh core
apt remove certbot
snap install --classic certbot
ln -s /snap/bin/certbot /usr/bin/certbot
###########################################################
# Crontab
###########################################################
echo "Configure cron jobs..." >> /var/www/html/status.txt
CRONTAB_FILE=/etc/cron.d/lumic.crontab
touch $CRONTAB_FILE
cat > "$CRONTAB_FILE" <<EOF
5 5 * * 5 certbot renew --nginx --non-interactive --post-hook "systemctl restart nginx.service"
* * * * * cd /var/www/html && php artisan schedule:run >> /dev/null 2>&1
* * * * * cd /var/www/html && php artisan queue:work --once >> /dev/null 2>&1
EOF
crontab $CRONTAB_FILE
###########################################################
# Installation complete
###########################################################
echo "Installation complete!"
