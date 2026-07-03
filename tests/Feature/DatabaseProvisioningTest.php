<?php

namespace Tests\Feature;

use App\Console\Commands\CreateDatabaseCommand;
use App\Models\Database;
use App\Models\Server;
use Illuminate\Support\Facades\Artisan;
use Tests\TestCase;

class DatabaseProvisioningTest extends TestCase
{
    public function test_database_sql_uses_idempotent_statements(): void
    {
        $database = new Database([
            'name' => 'example_db',
            'username' => 'example_user',
            'password' => 'secret',
        ]);

        $this->assertSame([
            'CREATE DATABASE IF NOT EXISTS `example_db`;',
            "CREATE USER IF NOT EXISTS 'example_user'@'localhost' IDENTIFIED BY 'secret';",
            "GRANT ALL PRIVILEGES ON `example_db`.* TO 'example_user'@'localhost';",
            'FLUSH PRIVILEGES;',
        ], CreateDatabaseCommand::sqlStatements($database));
    }

    public function test_database_route_creates_record_and_calls_provisioning_command(): void
    {
        $server = Server::create([
            'domain' => 'example.com',
            'name' => 'example-com',
            'path' => 'public',
        ]);

        Artisan::shouldReceive('call')
            ->once()
            ->with('db:create', \Mockery::on(fn ($payload) => isset($payload['database'])))
            ->andReturn(0);

        $this->post('/servers/' . $server->id . '/db', [
            'name' => 'example_db',
            'username' => 'example_user',
            'password' => 'secret',
        ]);

        $this->assertResponseStatus(302);
        $this->assertSame(1, Database::count());
        $this->assertSame('example_db', Database::first()->name);
    }

    public function test_database_route_rejects_invalid_names(): void
    {
        $server = Server::create([
            'domain' => 'example.com',
            'name' => 'example-com',
            'path' => 'public',
        ]);

        $this->post('/servers/' . $server->id . '/db', [
            'name' => 'bad-name',
            'username' => 'example_user',
            'password' => 'secret',
        ]);

        $this->assertResponseStatus(302);
        $this->assertSame(0, Database::count());
    }
}
