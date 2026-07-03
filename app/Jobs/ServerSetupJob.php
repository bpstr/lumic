<?php

namespace App\Jobs;

use App\Models\Server;
use Illuminate\Support\Facades\Artisan;

class ServerSetupJob extends Job
{
    public ?Server $server;

    public function __construct(?Server $server = null)
    {
        $this->server = $server;
    }

    public function handle()
    {
        foreach ($this->serversToSetup() as $server) {
            $this->setupServer($server);
        }
    }

    private function serversToSetup()
    {
        if ($this->server) {
            return collect([$this->server]);
        }

        $blocks = collect(scandir(storage_path('blocks')) ?: [])
            ->filter(fn ($item) => str_ends_with($item, '.conf'))
            ->map(fn ($item) => substr($item, 0, -5));

        return Server::whereIn('name', $blocks)->get();
    }

    private function setupServer(Server $server): void
    {
        $server->update(['setup_status' => 'running']);
        $server->appendSetupLog('Setup started.');

        try {
            Artisan::call('dir:prepare', ['server' => $server->id]);
            $server->appendSetupLog('Directories prepared.');

            $this->ensureLogDirectory($server);
            $this->installNginxConfig($server);
            $server->appendSetupLog('Nginx configuration installed.');

            foreach ($server->databases as $database) {
                Artisan::call('db:create', ['database' => $database->id]);
                $server->appendSetupLog('Database provisioned: '.$database->name);
            }

            Artisan::call('ssl:certificate', ['server' => $server->id]);
            $server->appendSetupLog('SSL certificate command completed.');

            Artisan::call('template:install', ['server' => $server->id]);
            $server->appendSetupLog('Template installed.');

            if (Artisan::call('nginx:restart') !== 0) {
                throw new \RuntimeException('Nginx restart failed.');
            }
            $server->appendSetupLog('Nginx restarted.');
            $server->update(['setup_status' => 'completed']);
        } catch (\Throwable $throwable) {
            $server->appendSetupLog('Setup failed: '.$throwable->getMessage());
            $server->update(['setup_status' => 'failed']);
        }
    }

    private function ensureLogDirectory(Server $server): void
    {
        $projectLogPath = sprintf('%s/%s/', getenv('NGINX_LOG_PATH'), $server->name);
        if (!is_dir($projectLogPath)) {
            mkdir($projectLogPath, 0755, true);
        }
    }

    private function installNginxConfig(Server $server): void
    {
        $source = storage_path('blocks/'.$server->name.'.conf');
        if (!is_file($source)) {
            $server->appendSetupLog('Nginx configuration already installed or not pending.');
            return;
        }

        if (Artisan::call('nginx:test') !== 0) {
            throw new \RuntimeException('Nginx configuration validation failed.');
        }

        rename($source, $server->nginx);
    }
}
