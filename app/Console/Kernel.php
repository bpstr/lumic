<?php

namespace App\Console;

use App\Console\Commands\CreateDatabaseCommand;
use App\Console\Commands\DatabaseUserCommand;
use App\Console\Commands\NginxConfigCommand;
use App\Console\Commands\RestartNginxCommand;
use App\Console\Commands\SslCertificateCommand;
use Illuminate\Console\Scheduling\Schedule;
use Laravel\Lumen\Console\Kernel as ConsoleKernel;

class Kernel extends ConsoleKernel
{
    /**
     * The Artisan commands provided by your application.
     *
     * @var array
     */
    protected $commands = [
        NginxConfigCommand::class,
        RestartNginxCommand::class,
        CreateDatabaseCommand::class,
        SslCertificateCommand::class,
        DatabaseUserCommand::class,
    ];

    /**
     * Define the application's command schedule.
     *
     * @param  \Illuminate\Console\Scheduling\Schedule  $schedule
     * @return void
     */
    protected function schedule(Schedule $schedule)
    {
        //
    }
}
