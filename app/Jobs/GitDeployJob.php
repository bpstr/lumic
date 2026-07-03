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
    public function __construct($server)
    {
        $this->server = $server;
    }

    /**
     * Execute the job.
     *
     * @return void
     */
    public function handle()
    {
        if (!$this->server->git) {
            $this->appendDeployLog('Deploy skipped: no Git repository configured.');
            return;
        }

        $this->appendDeployLog('User triggered deploy.');
        Artisan::call('git:deploy', ['server' => $this->server->id]);
    }

    private function appendDeployLog(string $line): void
    {
        $log = $this->server->deploy_log;
        $dir = dirname($log);
        if (!is_dir($dir)) {
            mkdir($dir, 0755, true);
        }

        file_put_contents($log, '[' . date('Y-m-d H:i:s') . '] ' . $line . PHP_EOL, FILE_APPEND);
    }
}
