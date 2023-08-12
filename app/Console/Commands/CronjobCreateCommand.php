<?php

namespace App\Console\Commands;

use App\Models\Server;
use Illuminate\Console\Command;

class CronjobCreateCommand extends Command
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'nginx:config {server}';

    /**
     * The console command description.
     *
     * @var string
     */
    protected $description = 'Manage Nginx configuration files';

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
        $server = $this->argument('server');
        if (!$server instanceof Server) {
            $server = Server::find($this->argument('server'));
        }

        $config = view(sprintf('sample.nginx-%s', $server->template ?? 'default'), compact('server'));
        file_put_contents(storage_path(sprintf('blocks/%s.conf', $server->name)), $config);

        $this->info('Created configuration: '.$server->nginx);
        return 1;
    }
}
