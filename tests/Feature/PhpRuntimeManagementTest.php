<?php

namespace Tests\Feature;

use App\Models\Server;
use App\Support\PhpRuntime;
use App\Support\ServerInput;
use Tests\TestCase;

class PhpRuntimeManagementTest extends TestCase
{
    public function test_unsupported_php_version_is_rejected(): void
    {
        $this->expectException(\InvalidArgumentException::class);
        ServerInput::assertPhpVersion('7.4');
    }

    public function test_php_socket_accessor_uses_selected_version(): void
    {
        $server = new Server(['php' => '8.2']);

        $this->assertSame('/var/run/php/php8.2-fpm.sock', $server->php_fpm_socket);
    }

    public function test_php_runtime_reports_configured_versions(): void
    {
        putenv('AVAILABLE_PHP_VERSIONS=8.1,8.2');

        $versions = PhpRuntime::versions();

        $this->assertSame('8.1', $versions[0]['version']);
        $this->assertArrayHasKey('installed', $versions[0]);
    }
}
