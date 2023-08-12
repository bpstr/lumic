@extends('layouts.details')

@section('details')
    <div class="row">
        <div class="col-md-4">
            <div class="border p-3 my-2">

                <form method="post" action="/servers/{{ $server->id }}/update">


                    <div class="mb-3">
                        <label for="gitRepository" class="form-label">Git repository</label>
                        <input type="text" name="git" class="form-control rounded-0" value="{{ $server->git }}" id="gitRepository" placeholder="git@github.com:bpstr/lumic.git">
                        <small class="text-muted">Use a password-protected SSH key.</small>
                    </div>

                    <div class="mb-3">
                        <label for="branchName" class="form-label">Branch name</label>
                        <input type="text" name="branch" class="form-control rounded-0" value="{{ $server->branch }}" id="branchName" placeholder="main">
                    </div>

                    <button type="submit" class="btn btn-primary rounded-0">Submit</button>
                </form>

            </div>
        </div>
        <div class="col-md-8">
            <div class="border p-3 my-3">
                <small class="d-block fw-bold text-uppercase mb-3">Deployment logs</small>
                <iframe src="/servers/{{ $server->id }}/deploy/logs" class="text-white bg-light border w-100" id="console"></iframe>
            </div>

        </div>
    </div>

@endsection
