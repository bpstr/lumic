<?php

namespace App\Console\Commands;

use App\Console\CommandBase;
use App\Models\Server;
use Illuminate\Console\Command;

class GitDeployCommand extends CommandBase
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'git:deploy {server}';

    /**
     * The console command description.
     *
     * @var string
     */
    protected $description = 'Deploy git repository to server';

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
        $server = $this->getServer();


        $git = $server->git;
        $token = getenv('GITHUB_TOKEN');
        $git = str_replace('git@github.com:', "https://oauth2:$token@github.com/", $git);

        if (!is_dir('/var/git/'.$server->name)) {
            static::exec('cd /var/git && GIT_SSH_COMMAND="ssh -o StrictHostKeyChecking=no" git clone '.$git.' '.$server->name . ' >> '.$server->deploy_log.' 2>&1');
        }

        static::exec('cd /var/git/'.$server->name.' && git reset --hard HEAD >> '.$server->deploy_log.' 2>&1');
        static::exec('cd /var/git/'.$server->name.' && git pull origin >> '.$server->deploy_log.' 2>&1');
        static::exec('cd /var/git/'.$server->name.' && git checkout main >> '.$server->deploy_log.' 2>&1');

        $commit = trim(static::exec('cd /var/git/'.$server->name.' && git log --pretty="%h" -n1 HEAD'));
//        $server->update(['commit' => $commit]);

        $this->info('Deployed commit: '.$commit);

        $data = file($server->deploy_log);
        $line = $data[count($data)-1];
        if (str_starts_with($line, "Your branch is up to date with 'origin/main'")) {
            $this->info('Already up to date.');
        }

        $this->info('Deploying...');
        return $this->deploy($server);
    }

    public function deploy($server) {
        if (file_exists('/var/git/'.$server->name.'/.lumic/hooks/pre-deploy.sh')) {
            static::exec('chmod +x ./.lumic/hooks/pre-deploy.sh >> '.$server->deploy_log.' 2>&1');
            static::exec('cd /var/git/'.$server->name.' && ./.lumic/hooks/pre-deploy.sh >> '.$server->deploy_log.' 2>&1');
        }

        $exclude_list = '';
        if (file_exists('/var/git/'.$server->name.'/.lumic/excluded.lst')) {
            $exclude_list = ' --exclude-from={'.'} ';
        }

        static::exec('rsync -av --exclude-from=/var/www/html/resources/lists/default-excluded.lst '.$exclude_list.' /var/git/'.$server->name.'/ /var/www/'.$server->name.'/ >> '.$server->deploy_log.' 2>&1');

        if (file_exists('/var/git/'.$server->name.'/.lumic/hooks/post-deploy.sh')) {
            static::exec('chmod +x /var/git/'.$server->name.'/.lumic/hooks/post-deploy.sh >> '.$server->deploy_log.' 2>&1');
            static::exec('cd /var/git/'.$server->name.' && ./.lumic/hooks/post-deploy.sh >> '.$server->deploy_log.' 2>&1');
        }

        return 1;
    }
}
