<?php

namespace App\Console\Commands;

use App\Console\CommandBase;
use App\Models\FtpUser;

class CreateFtpUserCommand extends CommandBase
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'ftp:create-user {ftpUser}';

    /**
     * The console command description.
     *
     * @var string
     */
    protected $description = 'Create an SFTP system user';

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
        $ftpUser = $this->argument('ftpUser');
        if (!$ftpUser instanceof FtpUser) {
            $ftpUser = FtpUser::find($ftpUser);
        }

        if (!$ftpUser) {
            throw new \InvalidArgumentException('SFTP user record not found.');
        }

        static::exec(self::useraddCommand($ftpUser));

        $this->info('Created SFTP user: '.$ftpUser->username);
        return 1;
    }

    public static function useraddCommand(FtpUser $ftpUser): array
    {
        return [
            'useradd',
            '-m',
            '-d',
            $ftpUser->home,
            '-s',
            $ftpUser->shell,
            $ftpUser->username,
        ];
    }
}
