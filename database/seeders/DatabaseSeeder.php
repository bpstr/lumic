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
            'name' => 'localhost',
            'domain' => 'localhost',
        ]);
    }
}
