<?php

namespace Tests\Feature;

use App\Console\Commands\CreateFtpUserCommand;
use App\Models\FtpUser;
use App\Models\Server;
use Illuminate\Support\Facades\Artisan;
use Tests\TestCase;

class FtpUserManagementTest extends TestCase
{
    public function test_sftp_user_route_validates_and_stores_user(): void
    {
        putenv('DOCROOT_PATH=/tmp/www');
        $server = Server::create([
            'domain' => 'example.com',
            'name' => 'example-com',
            'path' => 'public',
        ]);

        Artisan::shouldReceive('call')
            ->once()
            ->with('ftp:create-user', \Mockery::on(fn ($payload) => isset($payload['ftpUser'])))
            ->andReturn(0);

        $this->post('/servers/' . $server->id . '/ftp', [
            'username' => 'deploy_user',
            'home' => '/tmp/www/example-com/public',
            'shell' => '/usr/sbin/nologin',
        ]);

        $this->assertResponseStatus(302);
        $this->assertSame(1, FtpUser::count());
    }

    public function test_useradd_command_does_not_use_hard_coded_user(): void
    {
        $ftpUser = new FtpUser([
            'username' => 'deploy_user',
            'home' => '/tmp/www/example-com/public',
            'shell' => '/usr/sbin/nologin',
        ]);

        $this->assertSame([
            'useradd',
            '-m',
            '-d',
            '/tmp/www/example-com/public',
            '-s',
            '/usr/sbin/nologin',
            'deploy_user',
        ], CreateFtpUserCommand::useraddCommand($ftpUser));
    }
}
