<?php

namespace Database\Seeders;

use App\Models\Server;
use Illuminate\Database\Seeder;

class DatabaseSeeder extends Seeder
{
    /**
     * Run the database seeds.
     *
     * @return void
     */
    public function run()
    {
        Server::create([
            'name' => 'home',
            'domain' => getenv('SERVER_IP'),
            'php' => '8.1',
            'template' => 'laravel',
        ]);
    }
}
