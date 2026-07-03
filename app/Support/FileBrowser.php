<?php

namespace App\Support;

use App\Models\Server;
use InvalidArgumentException;

class FileBrowser
{
    public static function resolve(Server $server, ?string $path): string
    {
        $root = realpath($server->docroot);
        if ($root === false) {
            throw new InvalidArgumentException('Server docroot is not readable.');
        }

        $path = trim((string) $path, '/');
        if ($path !== '' && (str_contains($path, '..') || str_starts_with($path, '/') || preg_match('/[\x00-\x1F\x7F]/', $path))) {
            throw new InvalidArgumentException('Invalid browser path.');
        }

        $target = $path === '' ? $root : realpath($root . '/' . $path);
        if ($target === false || ($target !== $root && !str_starts_with($target, $root . DIRECTORY_SEPARATOR))) {
            throw new InvalidArgumentException('Path escapes the server docroot.');
        }

        return $target;
    }

    public static function entries(string $path): array
    {
        $entries = [];
        foreach (scandir($path) ?: [] as $entry) {
            if ($entry === '.' || $entry === '..') {
                continue;
            }

            $full = $path . DIRECTORY_SEPARATOR . $entry;
            $entries[] = [
                'name' => $entry,
                'type' => is_dir($full) ? 'directory' : 'file',
                'size' => is_file($full) ? filesize($full) : null,
                'modified' => filemtime($full),
            ];
        }

        usort($entries, fn ($a, $b) => [$a['type'] !== 'directory', $a['name']] <=> [$b['type'] !== 'directory', $b['name']]);

        return $entries;
    }
}
