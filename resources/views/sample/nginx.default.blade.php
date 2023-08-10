server {

    listen 80;
    listen [::]:80;

    server_tokens off;

    server_name {{ $server->domain }};

    root {{ $server->directory }};

    client_max_body_size 256M;

    access_log /home/log/{{ $server->name }}/access.log;
    error_log /home/log/{{ $server->name }}/error.log;

    location ~ \.php$ {
        include snippets/fastcgi-php.conf;
        fastcgi_pass unix:/var/run/php/php{{ $server->php }}-fpm.sock;
    }

    location ~ /\.(?!well-known).* {
        deny all;
    }

}
