<?php

namespace App\Console\Commands;

use App\Console\CommandBase;

class TestNginxCommand extends CommandBase
{
    protected $signature = 'nginx:test';

    protected $description = 'Validate the Nginx configuration';

    public function handle()
    {
        static::exec(['nginx', '-t']);

        $this->info('Nginx configuration is valid.');
        return 0;
    }
}
