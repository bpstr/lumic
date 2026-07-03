@extends('layouts.details')

@section('details')
    <section class="py-2">
        <div class="border p-3 my-2">
            <div class="row">
                @forelse($server->ftpUsers as $ftpUser)
                    <div class="col-md-6 col-lg-4">
                        <div class="card my-3 rounded-0">
                            <span class="card-body text-decoration-none pe-auto">
                                <small class="fw-bold">username: <code>{{ $ftpUser->username }}</code></small><br>
                                <small>home: <code>{{ $ftpUser->home }}</code></small><br>
                                <small>shell: <code>{{ $ftpUser->shell }}</code></small><br>
                                <small>protocol: <code>SFTP</code></small>
                            </span>
                        </div>
                    </div>
                @empty
                    <div class="col">
                        <p class="text-muted mb-0">No SFTP users configured.</p>
                    </div>
                @endforelse
            </div>

            <hr>

            <form method="post" action="/servers/{{ $server->id }}/ftp" autocomplete="off">
                <small class="d-block fw-bold text-uppercase my-2">Create SFTP user</small>
                <div class="row">
                    <div class="col-md">
                        <input class="form-control form-control-sm rounded-0" name="username" type="text" placeholder="Username">
                    </div>
                    <div class="col-md">
                        <input class="form-control form-control-sm rounded-0" name="home" type="text" value="{{ $server->docroot }}" placeholder="Home directory">
                    </div>
                    <div class="col-md">
                        <select class="form-select form-select-sm rounded-0" name="shell">
                            <option value="/usr/sbin/nologin">SFTP only</option>
                            <option value="/bin/bash">Shell access</option>
                        </select>
                    </div>
                    <div class="col-md-auto">
                        <button type="submit" class="btn btn-sm btn-primary mb-3 rounded-0">Create user</button>
                    </div>
                </div>
            </form>
        </div>
    </section>
@endsection
