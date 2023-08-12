<?php

namespace App\Console\Commands;

use App\Console\CommandBase;
use Illuminate\Console\Command;
use Illuminate\Support\Facades\Blade;
use Illuminate\Support\Facades\File;
use Illuminate\View\View;

class GenerateCronJobTable extends CommandBase
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'cron:table';

    /**
     * The console command description.
     *
     * @var string
     */
    protected $description = 'Generate an HTML table for cron jobs in resources/views';

    public function handle()
    {
        $rawCronJobs = [];
        exec('crontab -l', $rawCronJobs);

        $output_path = resource_path('views/blocks/cronjob.blade.php');
        $cronJobs = $this->parseCronJobs($rawCronJobs);
        $table_content = view('sample.crontab', ['cronJobs' => $cronJobs]);
        if (File::put($output_path,$table_content)) {
            $this->info('HTML table generated successfully at ' . $output_path);
        } else {
            $this->error('Failed to generate the HTML table.');
        }
    }

    /**
     * Parse raw cron jobs into a structured format.
     *
     * @param  array $rawCronJobs
     * @return array
     */
    protected function parseCronJobs(array $rawCronJobs)
    {
        $parsedCronJobs = [];

        foreach ($rawCronJobs as $job) {
            if (preg_match('/^((?:\S+\s+){5})(\S.*)$/', $job, $matches)) {
                $timeFields = explode(' ', $matches[1]);
                $command = $matches[2];

                $parsedCronJobs[] = array_merge($timeFields, [$command]);
            }
        }

        return $parsedCronJobs;
    }
}
