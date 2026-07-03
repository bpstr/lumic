<?php

namespace Tests\Unit;

use App\Models\Server;
use Tests\TestCase;

class ServerModelTest extends TestCase
{
    public function test_server_path_accessors_use_configured_roots(): void
    {
        putenv('DOCROOT_PATH=/srv/www');
        putenv('GITROOT_PATH=/srv/git');
        putenv('NGINX_ROOT_PATH=/etc/nginx');

        $server = new Server([
            'name' => 'example-com',
            'path' => 'public',
        ]);

        $this->assertSame('/srv/www/example-com/public', $server->docroot);
        $this->assertSame('/srv/git/example-com/public', $server->gitroot);
        $this->assertSame('/etc/nginx/sites-enabled/example-com.conf', $server->nginx);
    }
}
