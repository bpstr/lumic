@php use Carbon\CarbonInterface; @endphp

<div class="card rounded-0">
    <div class="card-header">
        <h6 class="text-uppercase my-0 small">Server details</h6>
    </div>
    <div class="card-body">

        <div class="row">
            <div class="col-md-5 col-lg-4">
                <h6>Server configuration</h6>
                <p class="text-nowrap">
                    <small>PHP Version: <code>{{ $server->php }}</code></small><br>
                    <small>SSL Certification: <code>{{ $server->ssl instanceof CarbonInterface ? $server->ssl->format('Y-m-d') : $server->ssl ?? '—' }}</code></small> <a href="/servers/{{ $server->id }}/renew" class="badge bg-light text-dark text-decoration-none border rounded-0">Renew</a> <br>
                    <small>Docroot: <code>{{ $server->directory }}</code></small><br>
                </p>
            </div>
            <div class="col-md-3">
                <h6>Primary database</h6>

                <p class="text-nowrap">
                    <small>Name: <code>{{ $server->database->name ?? '–' }}</code></small><br>
                    <small>Username: <code>{{ $server->database->username ?? '–' }}</code></small><br>
                    <small>Password: <code>{{ $server->database->password ?? '–' }}</code></small><br>
                </p>
            </div>
            <div class="col-md-3">
                <h6>FTP Access</h6>

                <p class="text-nowrap">
                    <small>FTP Host: <code>localhosthu.ftp.pixel24.hu</code></small><br>
                    <small>Username: <code>localhosthu</code></small><br>
                    <small>Password: <code>12k338jd84lkd93</code></small><br>
                </p>


            </div>
            <div class="col-md-2">
                <h6>Git Repo</h6>
                <div class="mb-1">
                <span>Current commit:</span><br>
                <code>{{ $server->commit ?? '–' }}</code>
                </div>
                <a href="/servers/{{ $server->id }}/deploy/trigger" class="btn btn-outline-dark btn-sm rounded-0">Deploy</a>
            </div>
        </div>


    </div>
    <div class="card-footer">
        <p class="small text-muted m-0">
                <span class="border-end pe-3 me-3">
                <b>Databases:</b>
                {{ $server->databases->count() }}
                </span>
            <span class="border-end pe-3 me-3">
                <b>Git repository:</b>
                {{ $server->git }}
                </span>
            <span>
                <b>Created:</b>
                {{ $server->created_at->format('Y-m-d H:i') }}
                </span>
        </p>
    </div>
</div>
