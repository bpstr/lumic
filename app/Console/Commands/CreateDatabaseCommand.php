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

        $database = $this->resolveDatabase();
        $secrets = [$rootpass, $database->password];

        foreach (self::sqlStatements($database) as $sql) {
            static::exec(['mysql', '-u', $rootuser, '-p'.$rootpass, '-e', $sql], $secrets);
        }

        $this->info('Database created.');
        return 1;
    }

    public function resolveDatabase(): Database
    {
        $database = $this->argument('database');
        if (!$database instanceof Database) {
            $database = Database::find($database);
        }

        if (!$database) {
            throw new \InvalidArgumentException('Database record not found.');
        }

        return $database;
    }

    public static function sqlStatements(Database $database): array
    {
        $name = self::identifier($database->name);
        $user = str_replace("'", "''", $database->username);
        $pass = str_replace("'", "''", $database->password);

        return [
            "CREATE DATABASE IF NOT EXISTS `{$name}`;",
            "CREATE USER IF NOT EXISTS '{$user}'@'localhost' IDENTIFIED BY '{$pass}';",
            "GRANT ALL PRIVILEGES ON `{$name}`.* TO '{$user}'@'localhost';",
            'FLUSH PRIVILEGES;',
        ];
    }

    private static function identifier(string $value): string
    {
        if (!preg_match('/^[A-Za-z0-9_]{1,64}$/', $value)) {
            throw new \InvalidArgumentException('Unsafe database identifier.');
        }

        return $value;
    }
}
