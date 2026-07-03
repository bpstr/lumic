<?php

namespace App\Support;

class AuthTokens
{
    public static function sessionToken(?string $username = null, ?string $password = null): string
    {
        $username ??= (string) getenv('ROOT_USER_NAME');
        $password ??= (string) getenv('ROOT_USER_PASS');

        return hash('sha256', $username . ':' . $password, true, ['salt' => getenv('APP_KEY')]);
    }

    public static function basicUsername(): string
    {
        return hash('sha256', getenv('ROOT_USER_NAME'), true, ['salt' => getenv('APP_KEY')]);
    }

    public static function basicPassword(): string
    {
        return hash('sha256', getenv('ROOT_USER_PASS'), true, ['salt' => getenv('APP_KEY')]);
    }

    public static function basicToken(): string
    {
        return base64_encode(self::basicUsername() . ':' . self::basicPassword());
    }
}
