@extends('layouts.auth')

@section('title', $server->name ?? 'Add new server')
@section('content')
<form method="POST" action="/servers/add" autocomplete="off">
    <div class="bg-light border">
        <div class="m-4 mx-4">
            <label for="domainName" class="form-label">Domain name</label>
            <input type="text" name="domain" class="form-control form-control-lg rounded-0" id="domainName" value="{{ $server->domain }}" autofocus autocomplete="off">
        </div>
    </div>

    <div class="border py-4 px-4 my-4">
        <div class="row">
            <div class="col-md-6">
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
                        <input type="text" name="path" class="form-control rounded-0" id="publicPath" placeholder="">
                    </div>
                    <p class="text-muted small ms-1 mt-1">Start without leading slashes</p>
                </div>

                <div class="mb-3">
                    <label for="gitRepository" class="form-label">Git repository</label>
                    <input type="text" name="git" class="form-control rounded-0" id="gitRepository" placeholder="git@github.com:bpstr/lumic.git">
                    <small class="text-muted">Use a password-protected SSH key.</small>
                </div>


            </div>

            <div class="col-md-6">
                <small class="d-block fw-bold text-uppercase my-3">Config template</small>

                <div class="list-group list-group-radio d-grid gap-2 border-0 mx-0">
                    @foreach(config('server.templates') as $config => $details)
                    <div class="position-relative">
                        <input class="form-check-input position-absolute top-50 end-0 me-3 fs-5" type="radio" name="template" id="{{ $config }}ServerTemplate" value="{{ $config }}" @if($details['selected'] ?? false) checked @endif>
                        <label class="list-group-item rounded-0 py-3 pe-5 overflow-hidden" for="{{ $config }}ServerTemplate">
                            <strong class="fw-semibold">{{ $details['name'] }}</strong>
                            <span class="d-block small opacity-75">{{ $details['hint'] }}</span>
                            <img src="/icons/{{ $config }}.svg" width="48" class="position-absolute end-0 top-50 opacity-25 me-5" style="margin-top: -24px">
                        </label>
                    </div>
                    @endforeach
                </div>
            </div>
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
