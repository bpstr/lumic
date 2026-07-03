<?php

namespace App\Console;

class CommandResult
{
    public function __construct(
        public readonly array|string $command,
        public readonly string $output,
        public readonly int $exitCode,
        public readonly ?float $timeout = null,
    ) {
    }

    public function commandForLog(array $secrets = []): string
    {
        $command = is_array($this->command) ? implode(' ', $this->command) : $this->command;

        return CommandBase::redact($command, $secrets);
    }
}
