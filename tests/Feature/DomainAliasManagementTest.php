<?php

namespace Tests\Feature;

use App\Models\DomainAlias;
use App\Models\Server;
use Tests\TestCase;

class DomainAliasManagementTest extends TestCase
{
    public function test_domain_alias_can_be_added_to_server(): void
    {
        $server = Server::create([
            'domain' => 'example.com',
            'name' => 'example-com',
            'path' => 'public',
        ]);

        $this->post('/servers/' . $server->id . '/domains', ['domain' => 'alias.example.com']);

        $this->assertResponseStatus(302);
        $this->assertSame(1, DomainAlias::count());
        $this->assertContains('alias.example.com', $server->fresh()->server_names);
    }

    public function test_invalid_alias_is_rejected(): void
    {
        $server = Server::create([
            'domain' => 'example.com',
            'name' => 'example-com',
            'path' => 'public',
        ]);

        $this->post('/servers/' . $server->id . '/domains', ['domain' => 'bad domain']);

        $this->assertResponseStatus(302);
        $this->assertSame(0, DomainAlias::count());
    }
}
