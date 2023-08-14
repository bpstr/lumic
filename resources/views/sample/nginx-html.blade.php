server {
    listen 80;
    server_name {{ $server->domain }} www.{{ $server->domain }};

    index index.html index.htm;
    charset utf-8;

    server_tokens off;

    root {{ getenv('DOCROOT_PATH') }}/{{ $server->name }}/{{ $server->path }};

    access_log {{ getenv('NGINX_LOG_PATH') }}{{ $server->name }}/access.log;
    error_log {{ getenv('NGINX_LOG_PATH') }}{{ $server->name }}/error.log;


    location / {
        try_files $uri $uri/ =404;
    }
}
