<?php

namespace Tests;

use Illuminate\Support\Facades\Artisan;
use Laravel\Lumen\Testing\TestCase as BaseTestCase;

abstract class TestCase extends BaseTestCase
{
    /**
     * Creates the application.
     *
     * @return \Laravel\Lumen\Application
     */
    public function createApplication()
    {
        return require __DIR__.'/../bootstrap/app.php';
    }

    protected function setUp(): void
    {
        parent::setUp();

        putenv('AVAILABLE_PHP_VERSIONS=8.1,8.2');
        putenv('ROOT_USER_NAME=root');
        putenv('ROOT_USER_PASS=secret');
        putenv('APP_KEY=testing-key');

        config([
            'database.default' => 'sqlite',
            'database.connections.sqlite.database' => ':memory:',
            'queue.default' => 'sync',
        ]);

        Artisan::call('migrate:fresh');
    }
}
