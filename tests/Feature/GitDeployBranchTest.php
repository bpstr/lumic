<?php

namespace Tests\Feature;

use App\Models\Server;
use App\Console\Commands\GitDeployCommand;
use App\Support\ServerInput;
use Tests\TestCase;

class GitDeployBranchTest extends TestCase
{
    public function test_branch_is_fillable_and_persists(): void
    {
        $server = Server::create([
            'domain' => 'example.com',
            'name' => 'example-com',
            'path' => 'public',
            'branch' => 'main',
        ]);

        $server->update(['branch' => 'release/2026']);

        $this->assertSame('release/2026', $server->fresh()->branch);
    }

    public function test_branch_validation_accepts_git_ref_names_and_rejects_unsafe_values(): void
    {
        ServerInput::assertBranch('feature/deploy-flow');

        $this->expectException(\InvalidArgumentException::class);
        ServerInput::assertBranch('../main;rm');
    }

    public function test_git_repository_commands_use_configured_branch(): void
    {
        $server = new Server([
            'name' => 'example-com',
            'path' => 'public',
        ]);

        $commands = GitDeployCommand::repositoryCommandLines($server, 'release/2026');

        $this->assertStringContainsString('git fetch origin release/2026', $commands[1]);
        $this->assertStringContainsString('git checkout release/2026', $commands[2]);
        $this->assertStringContainsString('git pull origin release/2026', $commands[3]);
    }
}
