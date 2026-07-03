@extends('layouts.auth')

@section('title', 'Database explorer')

@section('content')
    @isset($error)
        <div class="alert alert-warning rounded-0">{{ $error }}</div>
    @endisset

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
                        {{ $database['name'] }}
                    </td>
                    <td>{{ $database['size'] }}</td>
                    <td>{{ $database['tables'] }} tables</td>
                </tr>
            @endforeach
        </tbody>
    </table>
@endsection
