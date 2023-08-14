<?php

namespace App\Console\Commands;

use App\Console\CommandBase;
use App\Models\Server;
use Illuminate\Console\Command;

class PrepareDirectoriesCommand extends CommandBase
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
        $server = $this->getServer();

        $project_root_path = $server->directory;
        if (!is_dir($project_root_path)) {
            mkdir($project_root_path, 0755, true);
        }

        $project_git_repository = $server->gitroot;
        if ($server->git && !is_dir($project_git_repository)) {
            mkdir($project_git_repository, 0755, true);
        }

        $this->info('Created directory: ' . $project_root_path);
        return 1;
    }
}
