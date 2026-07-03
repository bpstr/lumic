<?php

namespace App\Console\Commands;

use App\Jobs\GitDeployJob;
use App\Jobs\ServerSetupJob;
use App\Models\Server;
use Illuminate\Console\Command;

class DoJobCommand extends Command
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'do:job';

    /**
     * The console command description.
     *
     * @var string
     */
    protected $description = 'Process deploy jobs for configured Git servers';

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
     * Execute the console command.s
     *
     * @return mixed
     */
    public function handle()
    {
        foreach (Server::whereNotNull('git')->get() as $server) {
            (new GitDeployJob($server))->handle();
        }


        $this->info('Deploy jobs processed.');
        return 1;
    }
}
