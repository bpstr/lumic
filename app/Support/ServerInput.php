<?php

namespace App\Support;

use Illuminate\Http\Request;
use Illuminate\Support\Str;
use InvalidArgumentException;

class ServerInput
{
    private const DOMAIN_PATTERN = '/^(?=.{1,253}$)(?!-)(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$/i';
    private const PATH_PATTERN = '/^[A-Za-z0-9._\/-]*$/';
    private const BRANCH_PATTERN = '/^(?!\/|.*(?:\.\.|\/\/|@\{|\\$|[\x00-\x20~^:?*\[\\\\]))[A-Za-z0-9._\/-]{1,255}(?<!\.lock)(?<!\/)(?<!\.)$/';

    public static function createPayload(Request $request): array
    {
        $payload = self::payload($request, true);
        $payload['name'] = self::serverName($payload['domain']);

        return $payload;
    }

    public static function updatePayload(Request $request): array
    {
        return self::payload($request, false);
    }

    public static function supportedPhpVersions(): array
    {
        return array_values(array_filter(array_map('trim', explode(',', (string) getenv('AVAILABLE_PHP_VERSIONS')))));
    }

    private static function payload(Request $request, bool $creating): array
    {
        $payload = [];

        foreach (['domain', 'path', 'php', 'git', 'branch', 'template'] as $field) {
            if ($request->has($field)) {
                $payload[$field] = trim((string) $request->input($field));
            }
        }

        foreach (['create_certificate', 'create_db_user', 'create_database'] as $field) {
            if ($request->has($field)) {
                $payload[$field] = $request->boolean($field);
            }
        }

        if ($creating && empty($payload['domain'])) {
            throw new InvalidArgumentException('Domain is required.');
        }

        if (isset($payload['domain'])) {
            self::assertDomain($payload['domain']);
        }

        if (isset($payload['path'])) {
            $payload['path'] = trim($payload['path'], '/');
            self::assertRelativePath($payload['path']);
        }

        if (isset($payload['php']) && $payload['php'] !== '') {
            self::assertPhpVersion($payload['php']);
        }

        if (isset($payload['template']) && $payload['template'] !== '') {
            self::assertTemplate($payload['template']);
        }

        if (isset($payload['branch']) && $payload['branch'] !== '') {
            self::assertBranch($payload['branch']);
        }

        return array_filter($payload, fn ($value) => $value !== '');
    }

    public static function assertDomain(string $domain): void
    {
        if (!preg_match(self::DOMAIN_PATTERN, $domain)) {
            throw new InvalidArgumentException('Enter a valid domain name.');
        }
    }

    public static function assertRelativePath(string $path): void
    {
        if ($path === '') {
            return;
        }

        if (
            str_contains($path, '..') ||
            str_starts_with($path, '/') ||
            preg_match('/[\x00-\x1F\x7F]/', $path) ||
            !preg_match(self::PATH_PATTERN, $path)
        ) {
            throw new InvalidArgumentException('Public path must be a safe relative path.');
        }
    }

    public static function assertPhpVersion(string $version): void
    {
        if (!in_array($version, self::supportedPhpVersions(), true)) {
            throw new InvalidArgumentException('Selected PHP version is not supported.');
        }
    }

    public static function assertTemplate(string $template): void
    {
        if (!array_key_exists($template, config('server.templates', []))) {
            throw new InvalidArgumentException('Selected server template is not supported.');
        }
    }

    public static function assertBranch(string $branch): void
    {
        if (!preg_match(self::BRANCH_PATTERN, $branch)) {
            throw new InvalidArgumentException('Branch name is not safe.');
        }
    }

    public static function serverName(string $domain): string
    {
        return Str::slug(str_replace('.', '-', $domain));
    }
}
