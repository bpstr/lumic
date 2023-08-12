@extends('layouts.auth')

@section('title', $server->name ?? 'Add new server')
@section('content')
<form method="POST" action="/servers/add">
    <div class="bg-light border">
        <div class="m-4 mx-4">
            <label for="domainName" class="form-label">Domain name</label>
            <input type="text" name="domain" class="form-control form-control-lg rounded-0" id="domainName" value="{{ $server->domain }}" autofocus autocomplete="off">
        </div>
    </div>

    <div class="border py-4 px-4 my-4">
        <small class="d-block fw-bold text-uppercase mb-3">Initial configuration</small>

        <div class="form-check form-switch">
            <input class="form-check-input" name="crate_certificate" type="checkbox" role="switch" id="createSslCert" checked>
            <label class="form-check-label" for="createSslCert">Create SSL certificate</label>
        </div>
        <div class="form-check form-switch">
            <input class="form-check-input" name="create_db_user" type="checkbox" role="switch" id="createDatabaseUser" checked>
            <label class="form-check-label" for="createDatabaseUser">Create new database user</label>
        </div>
        <div class="form-check form-switch">
            <input class="form-check-input" name="create_database" type="checkbox" role="switch" id="createDatabase" checked>
            <label class="form-check-label" for="createDatabase">Create new database</label>
        </div>
        <div class="form-check form-switch">
            <input class="form-check-input" type="checkbox" role="switch" id="createFtpUser" disabled>
            <label class="form-check-label" for="createFtpUser">Create FTP user</label>
        </div>

        <div class="my-4">
            <small class="d-block fw-bold text-uppercase mb-3">PHP Version</small>
            @foreach(explode(',', getenv('AVAILABLE_PHP_VERSIONS')) as $index => $version)
                <div class="form-check form-check-inline">
                    <input class="form-check-input" type="radio" name="php" id="phpVersion{{ $index }}" value="{{ $version }}" @if($version == '8.1') checked @endif>
                    <label class="form-check-label" for="phpVersion{{ $index }}">{{ $version }}</label>
                </div>
            @endforeach
        </div>

        <small class="d-block fw-bold text-uppercase my-3">Server settings</small>

        <div class="mb-3">
            <label for="publicPath" class="form-label">Public path</label>
            <div class="input-group">
                <span class="input-group-text rounded-0" id="domainAddon">/var/www/example-site/</span>
                <input type="text" name="path" class="form-control rounded-0" id="publicPath" placeholder="public">
            </div>
            <p class="text-muted small ms-1 mt-1">Start without leading slashes</p>
        </div>

        <div class="mb-3">
            <label for="gitRepository" class="form-label">Git repository</label>
            <input type="text" name="git" class="form-control rounded-0" id="gitRepository" placeholder="git@github.com:bpstr/lumic.git">
            <small class="text-muted">Use a password-protected SSH key.</small>
        </div>

        <div class="form-group mb-3">
            <label for="configTemplate" class="form-label">Nginx config template</label>
            <select class="form-select rounded-0" name="template" id="configTemplate">
                <option value="default">Default PHP Config</option>
                <option value="laravel">Laravel standard</option>
                <option value="drupal">Drupal recommended</option>
            </select>
        </div>

        <button type="submit" class="btn btn-primary rounded-0">Submit</button>
    </div>
</form>
    <script>
        document.getElementById('domainName').addEventListener('input', function(e) {
            const userValue = e.target.value;
            const sanitizedValue = userValue.replace(/\./g, '-');
            document.getElementById('domainAddon').textContent = '/var/www/' + sanitizedValue + '/';
        });
    </script>
@endsection
