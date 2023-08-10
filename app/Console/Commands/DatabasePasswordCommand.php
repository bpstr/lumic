<?php

namespace App\Console\Commands;

use Illuminate\Console\Command;

class DatabasePasswordCommand extends Command
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'db:password';

    /**
     * The console command description.
     *
     * @var string
     */
    protected $description = 'Reset database password';

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

        $this->info('reset db pass.');
        return 1;
    }
}
