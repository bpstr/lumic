@extends('layouts.auth')

@section('title', 'Settings')

@section('content')



            <div class="border my-3 p-3 pb-0">
                <small class="d-block fw-bold text-uppercase mb-3">API credentials</small>

                <div class="row">
                    <div class="col-md mb-3">
                        <label for="lumicAuthToken" class="form-label"><code>Authorization: Basic</code></label>
                        <input type="text" name="branch" class="form-control rounded-0" value="{{ $deploy_token }}" id="lumicAuthToken" disabled>
                    </div>
                    <div class="col-md mb-3">
                        <label for="lumicStatusUrl" class="form-label"><code>Status endpoint</code></label>
                        <input type="text" name="branch" class="form-control rounded-0" value="{{ getenv('APP_URL') }}/api/status" id="lumicStatusUrl" disabled>
                    </div>
                </div>

            </div>
@endsection
