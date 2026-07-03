<?php

namespace App\Console\Commands;

use App\Models\Cronjob;
use Illuminate\Console\Command;

class CronjobCreateCommand extends Command
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'cron:create {cronjob}';

    /**
     * The console command description.
     *
     * @var string
     */
    protected $description = 'Render a managed cron job line';

    /**
     * Create a new command instance.
     *
     * @return void
     */
    public function __construct()
    {
        parent::__construct();
    }

    /**
     * Execute the console command.
     *
     * @return mixed
     */
    public function handle()
    {
        $cronjob = $this->argument('cronjob');
        if (!$cronjob instanceof Cronjob) {
            $cronjob = Cronjob::find($this->argument('cronjob'));
        }

        if (!$cronjob) {
            throw new \InvalidArgumentException('Cron job not found.');
        }

        $this->line($cronjob->expression() . ' ' . $cronjob->command);
        return 1;
    }
}
