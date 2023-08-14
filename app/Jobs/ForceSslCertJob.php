<?php

namespace App\Jobs;

use App\Models\Server;
use Illuminate\Queue\SerializesModels;
use Illuminate\Support\Facades\Artisan;

class ForceSslCertJob extends Job
{
    public Server $server;

    /**
     * Create a new job instance.
     *
     * @return void
     */
    public function __construct(Server $server)
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
        Artisan::call('ssl:certificate', ['server' => $this->server->id, 'force' => true]);
    }
}
