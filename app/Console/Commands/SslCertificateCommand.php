<?php

namespace App\Console\Commands;

use App\Console\CommandBase;
use App\Models\Server;

class SslCertificateCommand extends CommandBase
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'ssl:certificate {server}';

    /**
     * The console command description.
     *
     * @var string
     */
    protected $description = 'Create database if not exists';

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
        $server = Server::find($this->argument('server'));

        $webmaster=getenv('WEBMASTER_EMAIL');
        $domains = implode(',', [
            $server->domain,
            'www.' . $server->domain,
        ]);

        $this->info('Creating SSL certificate...');
        try {
            self::exec("certbot --nginx --non-interactive --agree-tos -m $webmaster --domains=$domains --expand");
        }
        catch (\Exception $exception) {
            $this->error($exception->getMessage());
            throw $exception;
        }
        $server->update(['ssl' => date('Y-m-d H:i:s')]);

        return 1;
    }
}
