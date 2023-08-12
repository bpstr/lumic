<?php

namespace App\Console;

use App\Console\Commands\CreateDatabaseCommand;
use App\Console\Commands\CreateFtpUserCommand;
use App\Console\Commands\DatabaseUserCommand;
use App\Console\Commands\DoJobCommand;
use App\Console\Commands\GenerateCronJobTable;
use App\Console\Commands\GitDeployCommand;
use App\Console\Commands\HtmlTemplateCommand;
use App\Console\Commands\NginxConfigCommand;
use App\Console\Commands\PrepareDirectoriesCommand;
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
        DoJobCommand::class,
        PrepareDirectoriesCommand::class,
        NginxConfigCommand::class,
        SslCertificateCommand::class,
        RestartNginxCommand::class,
        CreateDatabaseCommand::class,
        DatabaseUserCommand::class,
        GenerateCronJobTable::class,
        HtmlTemplateCommand::class,
        GitDeployCommand::class,
        CreateFtpUserCommand::class,
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
