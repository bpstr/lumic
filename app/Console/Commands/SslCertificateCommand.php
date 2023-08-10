<?php

namespace App\Console\Commands;

use Illuminate\Console\Command;

class SslCertificateCommand extends Command
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'ssl:certificate';

    /**
     * The console command description.
     *
     * @var string
     */
    protected $description = 'Create database if not exists';

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

        $this->info('create db.');
        return 1;
    }
}
