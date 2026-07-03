<?php

namespace Tests\Feature;

use App\Models\Cronjob;
use App\Models\Server;
use Tests\TestCase;

class CronManagementTest extends TestCase
{
    public function test_cron_job_can_be_created_for_server(): void
    {
        $server = Server::create([
            'domain' => 'example.com',
            'name' => 'example-com',
            'path' => 'public',
        ]);

        $this->post('/servers/' . $server->id . '/cron', [
            'IspCron' => [
                'command' => '/usr/bin/php artisan schedule:run',
                'run_min' => '*/5',
                'run_hour' => '*',
                'run_mday' => '*',
                'run_month' => '*',
                'run_wday' => '*',
            ],
        ]);

        $this->assertResponseStatus(302);
        $this->assertSame(1, Cronjob::count());
        $this->assertSame('*/5', Cronjob::first()->minute);
    }

    public function test_invalid_cron_expression_is_rejected(): void
    {
        $server = Server::create([
            'domain' => 'example.com',
            'name' => 'example-com',
            'path' => 'public',
        ]);

        $this->post('/servers/' . $server->id . '/cron', [
            'IspCron' => [
                'command' => '/usr/bin/php artisan schedule:run',
                'run_min' => 'bad value',
            ],
        ]);

        $this->assertResponseStatus(302);
        $this->assertSame(0, Cronjob::count());
    }
}
