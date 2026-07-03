<?php

namespace App\Support;

class PhpRuntime
{
    public static function versions(): array
    {
        return array_map(fn ($version) => [
            'version' => $version,
            'socket' => self::socket($version),
            'installed' => is_file(self::socket($version)),
        ], ServerInput::supportedPhpVersions());
    }

    public static function extensions(): array
    {
        $output = shell_exec('php -m 2>/dev/null') ?: '';

        return array_values(array_filter(array_map('trim', explode("\n", $output)), fn ($line) => $line !== '' && !str_starts_with($line, '[')));
    }

    public static function socket(string $version): string
    {
        return '/var/run/php/php'.$version.'-fpm.sock';
    }
}
