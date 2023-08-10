<?php

namespace App\Console\Commands;

use App\Console\CommandBase;

class RestartNginxCommand extends CommandBase
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'nginx:restart';

    /**
     * The console command description.
     *
     * @var string
     */
    protected $description = 'Send restart signal to Nginx';

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
        $this->info('Restarting nginx...');
        return static::exec('systemctl reload nginx');
    }
}
