@extends('layouts.details')

@section('details')
    <div class="border p-3 my-2">
        <small class="d-block fw-bold text-uppercase mb-3">{{ $relativePath ?: '/' }}</small>
        <table class="table">
            <thead>
            <tr>
                <th>Name</th>
                <th>Type</th>
                <th>Size</th>
                <th>Modified</th>
            </tr>
            </thead>
            <tbody>
            @forelse($entries as $entry)
                <tr>
                    <td>
                        @if($entry['type'] === 'directory')
                            <a href="/servers/{{ $server->id }}/files?path={{ urlencode(trim($relativePath . '/' . $entry['name'], '/')) }}">{{ $entry['name'] }}</a>
                        @else
                            {{ $entry['name'] }}
                        @endif
                    </td>
                    <td>{{ $entry['type'] }}</td>
                    <td>{{ $entry['size'] === null ? '-' : $entry['size'] }}</td>
                    <td>{{ date('Y-m-d H:i:s', $entry['modified']) }}</td>
                </tr>
            @empty
                <tr>
                    <td colspan="4" class="text-muted">No files found.</td>
                </tr>
            @endforelse
            </tbody>
        </table>
    </div>
@endsection
