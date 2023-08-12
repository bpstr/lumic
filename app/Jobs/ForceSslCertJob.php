<?php

namespace App\Jobs;

use App\Models\Server;
use Illuminate\Support\Facades\Artisan;

class ForceSslCertJob extends Job
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
        $blocks = scandir(storage_path('blocks'));
        $servers = [];
        foreach ($blocks as $key => $item) {
            if (!str_ends_with($item, '.conf')) {
                continue;
            }

            $server = Server::where('name', substr($item, 0, -5))->first();

            if (!$server) {
                continue;
            }

            var_dump($item);

            $servers[] = $server;

            $project_log_path = sprintf('%s/%s/', getenv('NGINX_LOG_PATH'), $server->name);
            if (!is_dir($project_log_path)) {
                mkdir($project_log_path, 0755, true);
            }

            rename(storage_path('blocks/'.$item), $server->nginx);

            Artisan::call('ssl:certificate', ['server' => $server->id]);
            Artisan::call('template:install', compact('server'));
        }

        Artisan::call('nginx:restart');


//        file_put_contents('asdf.txt', var_export(storage_path('blocks'), true));
    }
}
