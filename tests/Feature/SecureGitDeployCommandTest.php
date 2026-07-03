<?php

namespace Tests\Feature;

use App\Console\Commands\GitDeployCommand;
use App\Models\Server;
use Tests\TestCase;

class SecureGitDeployCommandTest extends TestCase
{
    public function test_git_url_validation_allows_github_urls_and_rejects_shell_input(): void
    {
        GitDeployCommand::assertSafeGitUrl('git@github.com:bpstr/lumic.git');
        GitDeployCommand::assertSafeGitUrl('https://github.com/bpstr/lumic.git');

        $this->expectException(\InvalidArgumentException::class);
        GitDeployCommand::assertSafeGitUrl('https://github.com/bpstr/lumic.git;rm -rf /');
    }

    public function test_rsync_command_includes_default_exclude_list(): void
    {
        putenv('GITROOT_PATH=/tmp/git');
        putenv('DOCROOT_PATH=/tmp/www');

        $server = new Server([
            'name' => 'example-com',
            'path' => 'public',
        ]);

        $command = GitDeployCommand::rsyncCommand($server);

        $this->assertSame('rsync', $command[0]);
        $this->assertContains('--exclude-from='.resource_path('lists/default-excluded.lst'), $command);
        $this->assertContains('/tmp/git/example-com/public/', $command);
        $this->assertContains('/tmp/www/example-com/public/', $command);
    }

    public function test_rsync_command_includes_custom_exclude_list_when_present(): void
    {
        $root = sys_get_temp_dir() . '/lumic-git-deploy-test-' . uniqid();
        putenv('GITROOT_PATH=' . $root . '/git');
        putenv('DOCROOT_PATH=' . $root . '/www');

        $server = new Server([
            'name' => 'example-com',
            'path' => 'public',
        ]);

        mkdir($server->gitroot . '/.lumic', 0755, true);
        file_put_contents($server->gitroot . '/.lumic/excluded.lst', 'storage');

        $command = GitDeployCommand::rsyncCommand($server);

        $this->assertContains('--exclude-from=' . $server->gitroot . '/.lumic/excluded.lst', $command);
    }
}
