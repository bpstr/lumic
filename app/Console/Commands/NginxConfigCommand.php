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
    protected $description = 'Manage Nginx configuration files';

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
        $server = Server::first();
        $config = view('sample.nginx', compact('server'));
        file_put_contents($server->nginx, $config);

        $this->info('hello nginx.');
        return 1;
    }
}
