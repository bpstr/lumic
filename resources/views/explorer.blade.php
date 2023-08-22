@extends('layouts.auth')

@section('title', 'Database explorer')

@section('content')
    <table class="table">
        <thead>
            <tr>
                <th scope="col">#</th>
                <th scope="col">Name</th>
                <th scope="col">Size</th>
                <th scope="col"></th>
            </tr>
        </thead>
        <tbody>
            @foreach($databases as $index => $database)
                <tr>
                    <th scope="row">{{ $index }}</th>
                    <td>
                        <a href="/explorer/{{ $database['name'] }}" class="">
                            {{ $database['name'] }}
                        </a>
                    </td>
                    <td>{{ $database['size'] }}</td>
                    <td>{{ $database['tables'] }} tables</td>
                </tr>
            @endforeach
        </tbody>
    </table>
@endsection
