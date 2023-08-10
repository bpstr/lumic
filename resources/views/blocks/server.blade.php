
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
                    <small>Storage used: <code>{{ $server->storage_used }}</code></small><br>
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
        </div>


    </div>
    <div class="card-footer">
        <p class="small text-muted m-0">
                <span class="border-end pe-3 me-3">
                <b>Databases:</b>
                {{ $server->databases->count() }}
                </span>
            <span class="border-end pe-3 me-3">
                <b>Storage used:</b>
                {{ $server->storage_used }}
                </span>
            <span>
                <b>Created:</b>
                {{ $server->created_at->format('Y-m-d H:i') }}
                </span>
        </p>
    </div>
</div>
