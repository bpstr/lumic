@extends('layouts.details')

@section('details')
    <div class="border p-3 my-2">
        <div class="row">
            @forelse($server->aliases as $alias)
                <div class="col-md-4">
                    <div class="card my-2 rounded-0">
                        <div class="card-body">
                            <code>{{ $alias->domain }}</code>
                        </div>
                    </div>
                </div>
            @empty
                <div class="col">
                    <p class="text-muted mb-0">No aliases configured.</p>
                </div>
            @endforelse
        </div>

        <hr>

        <form method="post" action="/servers/{{ $server->id }}/domains">
            <small class="d-block fw-bold text-uppercase my-2">Add domain alias</small>
            <div class="row">
                <div class="col-md">
                    <input class="form-control form-control-sm rounded-0" name="domain" type="text" placeholder="alias.example.com">
                </div>
                <div class="col-md-auto">
                    <button type="submit" class="btn btn-sm btn-primary rounded-0">Add alias</button>
                </div>
            </div>
        </form>
    </div>
@endsection
