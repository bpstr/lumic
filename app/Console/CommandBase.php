<?php

namespace App\Console;

use App\Models\Server;
use Illuminate\Console\Command;
use Symfony\Component\Process\Process;

abstract class CommandBase extends Command
{

    public static function exec(array|string $cmd, array $secrets = [], ?string $cwd = null, ?float $timeout = null): string
    {
        $result = static::runCommand($cmd, secrets: $secrets, cwd: $cwd, timeout: $timeout);

        if ($result->exitCode !== 0) {
            throw new \RuntimeException(sprintf(
                '%s - %s',
                $result->commandForLog($secrets),
                static::redact($result->output, $secrets)
            ));
        }

        return $result->output;
    }

    public static function runCommand(
        array|string $cmd,
        array $env = [],
        array $secrets = [],
        ?string $cwd = null,
        ?float $timeout = null
    ): CommandResult {
        $process = is_array($cmd)
            ? new Process($cmd, $cwd, $env)
            : new Process(['sh', '-lc', $cmd], $cwd, $env);

        $processOutput = '';
        $captureOutput = function ($type, $line) use (&$processOutput) {
            $processOutput .= $line;
        };

        $process->setTimeout($timeout)
            ->run($captureOutput);

        return new CommandResult($cmd, $processOutput, $process->getExitCode() ?? 1, $timeout);
    }

    public static function redact(string $value, array $secrets = []): string
    {
        foreach ($secrets as $secret) {
            if ($secret === null || $secret === '') {
                continue;
            }

            $value = str_replace((string) $secret, '[redacted]', $value);
        }

        return $value;
    }

    protected function getServer() {
        $server = $this->argument('server');
        if (!$server instanceof Server) {
            $server = Server::find($this->argument('server'));
        }
        return $server;
    }


}
