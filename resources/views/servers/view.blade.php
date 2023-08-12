@extends('layouts.details')

@section('details')
    <section class="nginx py-2">
        <div class="card rounded-0">
            <div class="card-body">
                @if(file_exists($server->nginx))
                    <pre class="m-0">{{ file_get_contents($server->nginx) }}</pre>
                @else
                    <pre class="m-0">Config could not be located: {{ ($server->nginx)  }}</pre>
                @endif
            </div>
        </div>
    </section>
@endsection
