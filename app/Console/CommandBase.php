<?php

namespace App\Console;

use App\Models\Server;
use Illuminate\Console\Command;
use Symfony\Component\Process\Process;

abstract class CommandBase extends Command
{

    public static function exec($cmd): string
    {
        $process = Process::fromShellCommandline($cmd);

        $processOutput = '';

        $captureOutput = function ($type, $line) use (&$processOutput) {
            $processOutput .= $line;
        };

        $process->setTimeout(null)
            ->run($captureOutput);

        if ($process->getExitCode()) {
            throw new \RuntimeException($cmd . " - " . $processOutput);
        }

        return $processOutput;
    }

    protected function getServer() {
        $server = $this->argument('server');
        if (!$server instanceof Server) {
            $server = Server::find($this->argument('server'));
        }
        return $server;
    }


}
