<?php

namespace App\Console\Commands;

use App\Models\Server;
use Illuminate\Console\Command;

class NginxConfigCommand extends Command
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'nginx:config {server}';

    /**
     * The console command description.
     *
     * @var string
     */
    protected $description = 'Render an Nginx server configuration';

    /**
     * Create a new command instance.
     *
     * @return void
     */
    public function __construct()
    {
        parent::__construct();
    }

    /**
     * Execute the console command.
     *
     * @return mixed
     */
    public function handle()
    {
        $server = $this->argument('server');
        if (!$server instanceof Server) {
            $server = Server::find($this->argument('server'));
        }

        if ($server->php && !is_file($server->php_fpm_socket)) {
            throw new \RuntimeException('Selected PHP-FPM socket is missing: '.$server->php_fpm_socket);
        }

        $config = view(sprintf('sample.nginx-%s', $server->template ?? 'default'), compact('server'));
        file_put_contents(storage_path(sprintf('blocks/%s.conf', $server->name)), $config);

        $this->info('Created configuration: '.$server->nginx);
        return 1;
    }
}
