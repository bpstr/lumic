<?php

namespace Tests\Feature;

use App\Services\DatabaseExplorer;
use Laravel\Lumen\Application;
use Tests\TestCase;

class DatabaseExplorerTest extends TestCase
{
    public function test_explorer_renders_safe_error_when_service_fails(): void
    {
        $this->app->bind(DatabaseExplorer::class, fn (Application $app) => new class extends DatabaseExplorer {
            public function overview(): array
            {
                throw new \RuntimeException('Unable to connect to the database server.');
            }
        });

        $this->call('GET', '/explorer', [], [], [], [], null, [
            'HTTP_COOKIE' => 'auth=' . \App\Support\AuthTokens::sessionToken(),
        ]);

        $this->assertResponseOk();
        $this->assertStringContainsString('Unable to connect', $this->response->getContent());
    }
}
