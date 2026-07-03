<?php

namespace Tests\Feature;

use App\Jobs\ServerSetupJob;
use App\Models\Server;
use Illuminate\Support\Facades\Artisan;
use Illuminate\Support\Facades\Queue;
use Tests\TestCase;

class ServerInputValidationTest extends TestCase
{
    public function test_valid_server_input_creates_a_server_with_whitelisted_fields(): void
    {
        Queue::fake();
        Artisan::shouldReceive('call')
            ->once()
            ->with('nginx:config', \Mockery::on(fn ($payload) => $payload['server'] instanceof Server))
            ->andReturn(0);

        $this->post('/servers/add', [
            'domain' => 'example.com',
            'path' => 'public',
            'php' => '8.1',
            'template' => 'default',
            'git' => 'git@github.com:bpstr/lumic.git',
            'is_admin' => '1',
        ]);

        $this->assertResponseStatus(302);
        $this->assertSame(1, Server::count());
        $server = Server::first();
        $this->assertSame('example.com', $server->domain);
        $this->assertSame('example-com', $server->name);
        $this->assertSame('public', $server->path);
        $this->assertFalse(isset($server->is_admin));
        Queue::assertPushed(ServerSetupJob::class);
    }

    public function test_invalid_domain_is_rejected(): void
    {
        $this->post('/servers/add', [
            'domain' => 'not a domain',
            'php' => '8.1',
            'template' => 'default',
        ]);

        $this->assertResponseStatus(302);
        $this->assertSame(0, Server::count());
    }

    public function test_invalid_template_is_rejected(): void
    {
        $this->post('/servers/add', [
            'domain' => 'example.com',
            'php' => '8.1',
            'template' => 'missing',
        ]);

        $this->assertResponseStatus(302);
        $this->assertSame(0, Server::count());
    }

    public function test_invalid_php_version_is_rejected(): void
    {
        $this->post('/servers/add', [
            'domain' => 'example.com',
            'php' => '7.4',
            'template' => 'default',
        ]);

        $this->assertResponseStatus(302);
        $this->assertSame(0, Server::count());
    }

    public function test_unsafe_path_is_rejected(): void
    {
        $this->post('/servers/add', [
            'domain' => 'example.com',
            'path' => '../public;rm',
            'php' => '8.1',
            'template' => 'default',
        ]);

        $this->assertResponseStatus(302);
        $this->assertSame(0, Server::count());
    }
}
