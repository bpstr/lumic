<?php

namespace App\Jobs;

use App\Models\Server;
use Illuminate\Support\Facades\Artisan;

class GitDeployJob extends Job
{
    public Server $server;

    /**
     * Create a new job instance.
     *
     * @return void
     */
    public function __construct()
    {

    }

    /**
     * Execute the job.
     *
     * @return void
     */
    public function handle()
    {
        $servers = Server::whereNotNull('git')->get();
        foreach ($servers as $server) {
            if (!file_exists($server->deploy_log)) {
                continue;
            }

            $lines = file($server->deploy_log);
            if (count($lines) !== 1) {
                continue;
            }

            if (!str_starts_with($lines[0], 'User triggered deploy')) {
                continue;
            }

            Artisan::call('git:deploy', ['server' => $server->id]);
        }
    }
}
