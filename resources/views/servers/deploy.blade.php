@extends('layouts.details')

@section('details')
    <div class="row">
        <div class="col-md-4">
            <div class="border p-3 my-3">
                <small class="d-block fw-bold text-uppercase mb-3">Step 1: Git push prod</small>
                <p>git remote add production</p>
            </div>
        </div>
        <div class="col-md-4">
            <div class="border p-3 my-3">
                <small class="d-block fw-bold text-uppercase mb-3">Step 2: Receive git hook</small>
                <p>files are copied to the web dir</p>

                <form>
                    <div class="mb-3">
                        <label for="exampleFormControlTextarea1" class="form-label">Exclude list</label>
                        <textarea class="form-control" id="exampleFormControlTextarea1" rows="3"></textarea>
                    </div>
                </form>
            </div>
        </div>
        <div class="col-md-4">
            <div class="border p-3 my-3">
                <small class="d-block fw-bold text-uppercase mb-3">Step 3: Run post-deploy commaands</small>
                <p>./deploy.sh</p>
            </div>
        </div>
    </div>


@endsection
