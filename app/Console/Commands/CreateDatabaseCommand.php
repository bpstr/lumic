<?php

namespace App\Console\Commands;

use Illuminate\Console\Command;

class CreateDatabaseCommand extends Command
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'db:create';

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
// Bash
//# Create DB
//echo "CREATE DATABASE ${DB_NAME};" | mysql -u root -p"$DB_PASSWORD"
//# create DB user with password
//echo "CREATE USER '$DBUSER'@'localhost' IDENTIFIED BY '$DBUSER_PASSWORD';" | mysql -u root -p"$DB_PASSWORD"
//echo "GRANT ALL PRIVILEGES ON $DB_NAME.* TO '$DBUSER'@'localhost';" | mysql -u root -p"$DB_PASSWORD"
//echo "FLUSH PRIVILEGES;" | mysql -u root -p"$DB_PASSWORD"
        $this->info('create db.');
        return 1;
    }
}
