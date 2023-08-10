@extends('layouts.auth')

@section('title', $server->name)

@section('actions')

    <div class="btn-group me-2">
        <a href="#" class="btn btn-sm btn-outline-danger">Delete</a>
        <a href="https://{{ $server->domain }}" target="_blank" class="btn btn-sm btn-outline-dark">Open domain</a>
    </div>

@endsection

@section('content')
    @include('blocks.server')

    @php
        $tabs = [
            'servers/'. $server->id => 'Nginx config',
            'servers/'. $server->id .'/db' => 'Databases',
            'servers/'. $server->id .'/ftp' => 'FTP Accounts',
//            'servers/'. $server->id .'/deploy' => 'Deployment',
            'servers/'. $server->id .'/cron' => 'Cron jobs',
//            'servers/'. $server->id .'/domains' => 'Addon Domains',
        ];
        if ($server->template === 'laravel') {
            $tabs['servers/'. $server->id .'/artisan'] = 'Artisan UI';
        }
        if ($server->template === 'drupal') {
            $tabs['servers/'. $server->id .'/drush'] = 'Drush UI';
        }

        $path = request()->path();
    @endphp

    <nav class="nav mt-3 border-bottom">
        @foreach($tabs as $url => $tab)
            <a class="nav-link @if($path === $url) active border-bottom border-4 border-primary bg-light @endif" aria-current="page" href="/{{ $url }}">{{ $tab }}</a>
        @endforeach
    </nav>

    @yield('details')
@endsection
