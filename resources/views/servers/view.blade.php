@extends('layouts.details')

@section('details')
    <section class="nginx py-2">
        <div class="card rounded-0">
            <div class="card-body">
                <pre class="m-0">
server {
    listen 80;
    listen [::]:80;

    root /var/www/localhost-hu;
    index index.php index.html index.htm;

    server_name localhost-hu.localhost;

    location / {
        try_files $uri $uri/ =404;
    }

    location ~ \.php$ {
        include snippets/fastcgi-php.conf;
        fastcgi_pass unix:/var/run/php/php8.1-fpm.sock;
    }

    location ~ /\.ht {
        deny all;
    }
}</pre>
            </div>
        </div>

    </section>
@endsection
