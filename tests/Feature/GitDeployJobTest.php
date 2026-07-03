<?php

namespace Tests\Feature;

use App\Jobs\GitDeployJob;
use App\Models\Server;
use Illuminate\Support\Facades\Artisan;
use Tests\TestCase;

class GitDeployJobTest extends TestCase
{
    public function test_git_deploy_job_deploys_only_the_given_server(): void
    {
        putenv('DOCROOT_PATH=' . sys_get_temp_dir());

        $target = Server::create([
            'domain' => 'a.example.com',
            'name' => 'a-example-com',
            'path' => 'public',
            'git' => 'git@github.com:bpstr/a.git',
        ]);
        Server::create([
            'domain' => 'b.example.com',
            'name' => 'b-example-com',
            'path' => 'public',
            'git' => 'git@github.com:bpstr/b.git',
        ]);

        Artisan::shouldReceive('call')
            ->once()
            ->with('git:deploy', ['server' => $target->id])
            ->andReturn(0);

        (new GitDeployJob($target))->handle();
    }

    public function test_git_deploy_job_skips_missing_git_config(): void
    {
        putenv('DOCROOT_PATH=' . sys_get_temp_dir());

        $server = Server::create([
            'domain' => 'example.com',
            'name' => 'example-com',
            'path' => 'public',
        ]);

        Artisan::shouldReceive('call')->never();

        (new GitDeployJob($server))->handle();

        $this->assertStringContainsString('no Git repository configured', file_get_contents($server->deploy_log));
    }
}
