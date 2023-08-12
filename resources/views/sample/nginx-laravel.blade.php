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
