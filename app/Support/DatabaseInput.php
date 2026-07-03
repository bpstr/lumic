<?php

namespace App\Support;

use Illuminate\Http\Request;
use InvalidArgumentException;

class DatabaseInput
{
    private const NAME_PATTERN = '/^[A-Za-z0-9_]{1,64}$/';

    public static function payload(Request $request): array
    {
        $payload = [
            'name' => trim((string) $request->input('name')),
            'username' => trim((string) $request->input('username')),
            'password' => (string) $request->input('password'),
        ];

        self::assertName($payload['name'], 'database name');
        self::assertName($payload['username'], 'database username');
        self::assertPassword($payload['password']);

        return $payload;
    }

    public static function assertName(string $value, string $label): void
    {
        if (!preg_match(self::NAME_PATTERN, $value)) {
            throw new InvalidArgumentException("The {$label} may only contain letters, numbers, and underscores.");
        }
    }

    public static function assertPassword(string $password): void
    {
        if ($password === '' || strlen($password) > 255 || preg_match('/[\x00-\x1F\x7F]/', $password)) {
            throw new InvalidArgumentException('Database password must be present and must not contain control characters.');
        }
    }
}
