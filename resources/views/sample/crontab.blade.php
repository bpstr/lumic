<table class="table">
    <thead>
    <tr>
        <th scope="col">#</th>
        <th scope="col">Minute</th>
        <th scope="col">Hour</th>
        <th scope="col">Day of Month</th>
        <th scope="col">Month</th>
        <th scope="col">Day of Week</th>
        <th scope="col">Command</th>
    </tr>
    </thead>
    <tbody>
    @foreach($cronJobs as $index => $job)
        <tr>
            <th scope="row">{{ $index + 1 }}</th>
            <td>{{ $job[0] }}</td>
            <td>{{ $job[1] }}</td>
            <td>{{ $job[2] }}</td>
            <td>{{ $job[3] }}</td>
            <td>{{ $job[4] }}</td>
            <td>{{ $job[5] }}</td>
        </tr>
    @endforeach
    </tbody>
</table>
