<?php

namespace Tests\Feature;

use App\Models\Server;
use App\Support\FileBrowser;
use Tests\TestCase;

class FileBrowserTest extends TestCase
{
    public function test_file_browser_resolves_paths_inside_docroot(): void
    {
        $root = sys_get_temp_dir() . '/lumic-files-' . uniqid();
        mkdir($root . '/example-com/public/assets', 0755, true);
        putenv('DOCROOT_PATH=' . $root);
        $server = new Server(['name' => 'example-com', 'path' => 'public']);

        $this->assertSame(realpath($root . '/example-com/public/assets'), FileBrowser::resolve($server, 'assets'));
    }

    public function test_file_browser_rejects_traversal(): void
    {
        $root = sys_get_temp_dir() . '/lumic-files-' . uniqid();
        mkdir($root . '/example-com/public', 0755, true);
        putenv('DOCROOT_PATH=' . $root);
        $server = new Server(['name' => 'example-com', 'path' => 'public']);

        $this->expectException(\InvalidArgumentException::class);
        FileBrowser::resolve($server, '../secrets');
    }
}
