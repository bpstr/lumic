<?php

namespace App\Console\Commands;

use App\Models\Server;
use Illuminate\Console\Command;

class PrepareDirectoriesCommand extends Command
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'dir:prepare {server}';

    /**
     * The console command description.
     *
     * @var string
     */
    protected $description = 'Prepare directories for the project';

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
        $server = Server::find($this->argument('server'))->first();
        $project_root_path = $server->directory;
        if (!is_dir($project_root_path)) {
            mkdir($project_root_path, 0755, true);
        }

        $this->info('Created directory: ' . $project_root_path);
        return 1;
    }
}
