<?php

namespace Tests\Feature;

use App\Jobs\ServerSetupJob;
use App\Models\Server;
use Illuminate\Support\Facades\Artisan;
use Tests\TestCase;

class ServerSetupJobTest extends TestCase
{
    public function test_repeated_setup_without_pending_config_is_safe(): void
    {
        putenv('NGINX_LOG_PATH=' . sys_get_temp_dir() . '/lumic-nginx-logs');
        putenv('NGINX_ROOT_PATH=' . sys_get_temp_dir() . '/lumic-nginx');
        mkdir(getenv('NGINX_ROOT_PATH') . '/sites-enabled', 0755, true);

        $server = Server::create([
            'domain' => 'example.com',
            'name' => 'example-com',
            'path' => 'public',
        ]);

        Artisan::shouldReceive('call')->with('dir:prepare', ['server' => $server->id])->twice()->andReturn(0);
        Artisan::shouldReceive('call')->with('ssl:certificate', ['server' => $server->id])->twice()->andReturn(0);
        Artisan::shouldReceive('call')->with('template:install', ['server' => $server->id])->twice()->andReturn(0);
        Artisan::shouldReceive('call')->with('nginx:restart')->twice()->andReturn(0);

        (new ServerSetupJob($server))->handle();
        (new ServerSetupJob($server))->handle();

        $this->assertSame('completed', $server->fresh()->setup_status);
    }

    public function test_nginx_restart_is_skipped_when_config_validation_fails(): void
    {
        putenv('NGINX_LOG_PATH=' . sys_get_temp_dir() . '/lumic-nginx-logs');
        putenv('NGINX_ROOT_PATH=' . sys_get_temp_dir() . '/lumic-nginx');
        mkdir(getenv('NGINX_ROOT_PATH') . '/sites-enabled', 0755, true);
        mkdir(storage_path('blocks'), 0755, true);

        $server = Server::create([
            'domain' => 'example.com',
            'name' => 'example-com',
            'path' => 'public',
        ]);
        file_put_contents(storage_path('blocks/example-com.conf'), 'server {}');

        Artisan::shouldReceive('call')->with('dir:prepare', ['server' => $server->id])->once()->andReturn(0);
        Artisan::shouldReceive('call')->with('nginx:test')->once()->andReturn(1);
        Artisan::shouldReceive('call')->with('nginx:restart')->never();

        (new ServerSetupJob($server))->handle();

        $this->assertSame('failed', $server->fresh()->setup_status);
    }
}
