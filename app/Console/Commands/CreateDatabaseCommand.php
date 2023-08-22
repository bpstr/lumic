<?php

namespace App\Console\Commands;

use App\Console\CommandBase;
use App\Models\Database;
use App\Models\Server;

class CreateDatabaseCommand extends CommandBase
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'db:create {database}';

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
        $rootuser = getenv('MYSQL_ROOT_USER');
        $rootpass = getenv('MYSQL_ROOT_PASS');

        $database = $this->argument('database');
        if (!$database instanceof Database) {
            $database = Database::find($this->argument('database'));
        }

        $dbname = $database->name;
        $dbuser = $database->username;
        $dbpass = $database->password;

        static::exec('echo "CREATE DATABASE '.$dbname.';" | mysql -u '.$rootuser.' -p"'.$rootpass.'"');
        static::exec('echo "CREATE USER \''.$dbuser.'\'@\'localhost\' IDENTIFIED BY \''.$dbpass.'\';" | mysql -u '.$rootuser.' -p"'.$rootpass.'"');
        static::exec('echo "GRANT ALL PRIVILEGES ON '.$dbname.'.* TO \''.$dbuser.'\'@\'localhost\';" | mysql -u '.$rootuser.' -p"'.$rootpass.'"');
        static::exec('echo "FLUSH PRIVILEGES;" | mysql -u '.$rootuser.' -p"'.$rootpass.'"');

        $this->info('Database created.');
        return 1;
    }
}
