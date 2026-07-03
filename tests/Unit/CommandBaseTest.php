<?php

namespace Tests\Unit;

use App\Console\CommandBase;
use Tests\TestCase;

class CommandBaseTest extends TestCase
{
    public function test_run_command_returns_output_and_exit_code(): void
    {
        $result = TestCommand::runCommand(['php', '-r', 'echo "ok";']);

        $this->assertSame(0, $result->exitCode);
        $this->assertSame('ok', $result->output);
    }

    public function test_exec_throws_on_non_zero_exit(): void
    {
        $this->expectException(\RuntimeException::class);
        $this->expectExceptionMessage('failure');

        TestCommand::exec(['php', '-r', 'fwrite(STDERR, "failure"); exit(2);']);
    }

    public function test_exec_redacts_secrets_from_exception_messages(): void
    {
        try {
            TestCommand::exec(['php', '-r', 'fwrite(STDERR, "token-secret"); exit(1);'], ['token-secret']);
            $this->fail('Expected command failure.');
        } catch (\RuntimeException $exception) {
            $this->assertStringContainsString('[redacted]', $exception->getMessage());
            $this->assertStringNotContainsString('token-secret', $exception->getMessage());
        }
    }
}

class TestCommand extends CommandBase
{
    protected $signature = 'test:command-base';
}
