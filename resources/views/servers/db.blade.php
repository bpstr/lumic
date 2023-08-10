@extends('layouts.details')

@section('details')
    <section class="nginx py-2">
        <div class="card rounded-0">
            <div class="card-body">
                <div class="row">
                @foreach($server->databases as $database)
                    <div class="col-md-4 col-lg-3">
                        <div class="card my-3 rounded-0 relative">
                            <span class="card-body text-decoration-none pe-auto">
                                <small class="fw-bold">database: <code>{{ $database->name }}</code></small><br>
                                <small>username: <code>{{ $database->username }}</code></small><br>
                                <small>password: <code>{{ $database->password }}</code></small><br>
                                <small>hostname: <code>127.0.0.1</code></small>
                            </span>
                        </div>
                    </div>
                @endforeach
                </div>

                <hr>

                <form method="post" action="/servers/{{ $server->id }}/db" autocomplete="off">
                    <small class="d-block fw-bold text-uppercase my-2">Create new database</small>
                    <div class="row">
                        <div class="col-md">
                            <input class="form-control form-control-sm" name="name" type="text" placeholder="Database name">
                        </div>

                        <div class="col-md">
                            <input class="form-control form-control-sm" name="username" type="text" placeholder="Username">
                        </div>

                        <div class="col-md">
                            <input class="form-control form-control-sm" name="password" type="text" placeholder="Password">
                        </div>


                        <div class="col-md-auto">
                            <button type="submit" class="btn btn-sm btn-primary mb-3">Create database</button>
                        </div>


                    </div>
                </form>

            </div>
        </div>

    </section>
@endsection
