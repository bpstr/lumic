<?php

namespace App\Console\Commands;

use App\Console\CommandBase;
use Illuminate\Console\Command;
use Illuminate\Support\Facades\File;

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

        $cronJobs = $this->parseCronJobs($rawCronJobs);

        // Ensure that the blade template exists
        $templatePath = resource_path('views/cronjob.blade.php');
        if (!File::exists($templatePath)) {
            File::put($templatePath, $this->getCronJobTemplate());
        }

        $tableContent = view('cronjob-table', ['cronJobs' => $cronJobs])->render();

        $outputPath = resource_path('views/' . $this->argument('outputFile'));
        if (File::put($outputPath, $tableContent)) {
            $this->info('HTML table generated successfully at ' . $outputPath);
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


    /**
     * Get the default content for the cronjob-table blade file.
     *
     * @return string
     */
    protected function getCronJobTemplate()
    {
        return <<<BLADE
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
    @foreach(\$cronJobs as \$index => \$job)
    <tr>
      <th scope="row">{{ \$index + 1 }}</th>
      <td>{{ \$job[0] }}</td>
      <td>{{ \$job[1] }}</td>
      <td>{{ \$job[2] }}</td>
      <td>{{ \$job[3] }}</td>
      <td>{{ \$job[4] }}</td>
      <td>{{ \$job[5] }}</td>
    </tr>
    @endforeach
  </tbody>
</table>
BLADE;
    }
}
