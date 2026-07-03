<?php

namespace App\Console\Commands;

use App\Console\CommandBase;
use App\Models\Server;

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
        $branch = $server->branch ?: 'main';
        $git = (string) $server->git;

        self::assertSafeServer($server);
        self::assertSafeGitUrl($git);

        if (!is_dir(dirname($server->gitroot))) {
            mkdir(dirname($server->gitroot), 0755, true);
        }

        if (!is_dir($server->gitroot)) {
            $this->runAndLog(['git', 'clone', $git, $server->gitroot], dirname($server->gitroot));
        }

        foreach (self::repositoryCommands($branch) as $command) {
            $this->runAndLog($command, $server->gitroot);
        }

        $commit = trim($this->runAndLog(['git', 'log', '--pretty=%h', '-n1', 'HEAD'], $server->gitroot));
        $server->update(['commit' => $commit]);

        $this->info('Deployed commit: '.$commit);

        $this->info('Deploying...');
        return $this->deploy($server);
    }

    public static function repositoryCommandLines($server, string $branch): array
    {
        return array_map(fn ($command) => implode(' ', $command), self::repositoryCommands($branch));
    }

    public static function repositoryCommands(string $branch): array
    {
        return [
            ['git', 'reset', '--hard', 'HEAD'],
            ['git', 'fetch', 'origin', $branch],
            ['git', 'checkout', $branch],
            ['git', 'pull', 'origin', $branch],
        ];
    }

    public function deploy($server) {
        $preDeploy = $server->gitroot.'/.lumic/hooks/pre-deploy.sh';
        if (is_file($preDeploy)) {
            $this->runAndLog(['chmod', '+x', $preDeploy]);
            $this->runAndLog([$preDeploy], $server->gitroot);
        }

        $this->runAndLog(self::rsyncCommand($server));

        $postDeploy = $server->gitroot.'/.lumic/hooks/post-deploy.sh';
        if (is_file($postDeploy)) {
            $this->runAndLog(['chmod', '+x', $postDeploy]);
            $this->runAndLog([$postDeploy], $server->gitroot);
        }

        return 1;
    }

    public static function rsyncCommand($server): array
    {
        $command = [
            'rsync',
            '-av',
            '--exclude-from='.resource_path('lists/default-excluded.lst'),
        ];

        $customExclude = $server->gitroot.'/.lumic/excluded.lst';
        if (is_file($customExclude)) {
            $command[] = '--exclude-from='.$customExclude;
        }

        $command[] = rtrim($server->gitroot, '/') . '/';
        $command[] = rtrim($server->docroot, '/') . '/';

        return $command;
    }

    public static function assertSafeGitUrl(string $url): void
    {
        $ssh = preg_match('/^git@github\.com:[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+\.git$/', $url);
        $https = preg_match('/^https:\/\/github\.com\/[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+(?:\.git)?$/', $url);

        if (!$ssh && !$https) {
            throw new \InvalidArgumentException('Only GitHub SSH or HTTPS repository URLs are supported.');
        }
    }

    public static function assertSafeServer($server): void
    {
        if (!preg_match('/^[a-z0-9-]+$/', (string) $server->name)) {
            throw new \InvalidArgumentException('Unsafe server name for deploy.');
        }
    }

    private function runAndLog(array $command, ?string $cwd = null): string
    {
        $server = $this->getServer();
        $result = static::runCommand($command, cwd: $cwd);
        $line = '$ ' . $result->commandForLog() . PHP_EOL . $result->output;

        $dir = dirname($server->deploy_log);
        if (!is_dir($dir)) {
            mkdir($dir, 0755, true);
        }
        file_put_contents($server->deploy_log, $line, FILE_APPEND);

        if ($result->exitCode !== 0) {
            throw new \RuntimeException($result->commandForLog() . ' - ' . $result->output);
        }

        return $result->output;
    }
}
