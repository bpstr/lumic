<?php

namespace App\Console\Commands;

use App\Console\CommandBase;
use Illuminate\Console\Command;

class CreateFtpUserCommand extends CommandBase
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'ftp:create';

    /**
     * The console command description.
     *
     * @var string
     */
    protected $description = 'Create a new ftp user';

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
        static::exec('useradd -m ftp2 -s /bin/bash');


        $this->info('create db.');
        return 1;
    }
}
