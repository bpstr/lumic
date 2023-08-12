@extends('layouts.details')

@section('details')
    <div class="row">
        <div class="col-md-7 col-lg-8">
            <div class="border p-3 my-2">
                @include('blocks.cronjob')
            </div>
        </div>
        <div class="col-md-5 col-lg-4">
            <div class="border p-3 my-3">
                <form method="POST" action="/servers/{{ $server->id }}/cron">
                    <small class="d-block fw-bold text-uppercase mb-3">Add cron job</small>
                    <div class="form-group">
                        <label class="form-label" for="ispCronCommand">Command</label>
                        <input class="form-control rounded-0" type="text" name="IspCron[command]" value="{{ $server->directory }}" id="ispCronCommand" autocomplete="off" required>
                    </div>

                    <div class="form-group">
                        <label class="form-label" for="ispCronRunMin">Minute</label>
                        <input class="form-control rounded-0" type="text" name="IspCron[run_min]" value="*" id="ispCronRunMin" autocomplete="off">
                    </div>

                    <div class="form-group">
                        <label class="form-label" for="IspCron_run_hour">Hour</label>
                        <input class="form-control rounded-0" type="text" name="IspCron[run_hour]" value="*" id="IspCron_run_hour" autocomplete="off">
                    </div>

                    <div class="form-group">
                        <label class="form-label" for="IspCron_run_mday">Day of the Month</label>
                        <input class="form-control rounded-0" type="text" name="IspCron[run_mday]" value="*" id="IspCron_run_mday" autocomplete="off">
                    </div>

                    <div class="form-group">
                        <label class="form-label" for="IspCron_run_month">Month</label>
                        <input class="form-control rounded-0" type="text" name="IspCron[run_month]" value="*" id="IspCron_run_month" autocomplete="off">
                    </div>

                    <div class="form-group">
                        <label class="form-label" for="IspCron_run_wday">Day of the Week</label>
                        <input class="form-control rounded-0" type="text" name="IspCron[run_wday]" value="*" id="IspCron_run_wday" autocomplete="off">
                    </div>

                    <button type="submit" class="btn btn-primary rounded-0 mt-3">Submit</button>
                </form>

            </div>
        </div>
    </div>


@endsection
