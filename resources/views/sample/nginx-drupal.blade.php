server {
    listen 80;
    server_name {{ $server->domain }} www.{{ $server->domain }};

    index index.html index.php index.htm;
    charset utf-8;

    server_tokens off;

    root {{ getenv('DOCROOT_PATH') }}/{{ $server->name }}/{{ $server->path }};

    client_max_body_size 256M;

    access_log {{ getenv('NGINX_LOG_PATH') }}{{ $server->name }}/access.log;
    error_log {{ getenv('NGINX_LOG_PATH') }}{{ $server->name }}/error.log;

    location = /favicon.ico {
        log_not_found off;
        access_log off;
    }

    location = /robots.txt {
        allow all;
        log_not_found off;
        access_log off;
    }

    location ~* \.(txt|log)$ {
        allow 192.168.0.0/16;
        deny all;
    }

    location ~ \..*/.*\.php$ {
        return 403;
    }

    location ~ ^/sites/.*/private/ {
        return 403;
    }

    location ~ ^/sites/[^/]+/files/.*\.php$ {
        deny all;
    }

    location ~* ^/.well-known/ {
        allow all;
    }

    location ~ (^|/)\. {
        return 403;
    }

    location / {
        try_files $uri /index.php?$query_string;
    }

    location @rewrite {
        rewrite ^ /index.php;
    }

    location ~ /vendor/.*\.php$ {
        deny all;
        return 404;
    }

    location ~* \.(engine|inc|install|make|module|profile|po|sh|.*sql|theme|twig|tpl(\.php)?|xtmpl|yml)(~|\.sw[op]|\.bak|\.orig|\.save)?$|^(\.(?!well-known).*|Entries.*|Repository|Root|Tag|Template|composer\.(json|lock)|web\.config)$|^#.*#$|\.php(~|\.sw[op]|\.bak|\.orig|\.save)$ {
        deny all;
        return 404;
    }

    location ~ '\.php$|^/update.php' {
        fastcgi_split_path_info ^(.+?\.php)(|/.*)$;
        try_files $fastcgi_script_name =404;
        include fastcgi_params;
        fastcgi_param HTTP_PROXY "";
        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
        fastcgi_param PATH_INFO $fastcgi_path_info;
        fastcgi_param QUERY_STRING $query_string;
        fastcgi_intercept_errors on;
        fastcgi_pass unix:/var/run/php/php{{ $server->php }}-fpm.sock;
    }

    location ~* \.(js|css|png|jpg|jpeg|gif|ico|svg)$ {
        try_files $uri @rewrite;
        expires max;
        log_not_found off;
    }

    location ~ ^/sites/.*/files/styles/ { # For Drupal >= 7
        try_files $uri @rewrite;
    }

    location ~ ^(/[a-z\-]+)?/system/files/ { # For Drupal >= 7
        try_files $uri /index.php?$query_string;
    }

    if ($request_uri ~* "^(.*/)index\.php/(.*)") {
        return 307 $1$2;
    }
}
