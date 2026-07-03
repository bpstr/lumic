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
    @forelse($server->cronjobs as $cronjob)
        <tr>
            <th scope="row">{{ $cronjob->id }}</th>
            <td>{{ $cronjob->minute }}</td>
            <td>{{ $cronjob->hour }}</td>
            <td>{{ $cronjob->day_of_month }}</td>
            <td>{{ $cronjob->month }}</td>
            <td>{{ $cronjob->day_of_week }}</td>
            <td><code>{{ $cronjob->command }}</code></td>
        </tr>
    @empty
        <tr>
            <td colspan="7" class="text-muted">No cron jobs configured.</td>
        </tr>
    @endforelse
    </tbody>
</table>
