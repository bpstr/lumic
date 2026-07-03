<?php

namespace App\Support;

use App\Models\Server;
use Illuminate\Http\Request;
use InvalidArgumentException;

class FtpUserInput
{
    public static function payload(Request $request, Server $server): array
    {
        $username = trim((string) $request->input('username'));
        $home = trim((string) $request->input('home', $server->docroot));
        $shell = trim((string) $request->input('shell', '/usr/sbin/nologin'));

        if (!preg_match('/^[a-z_][a-z0-9_-]{0,31}$/', $username)) {
            throw new InvalidArgumentException('SFTP username is not valid.');
        }

        if ($home !== $server->docroot && !str_starts_with($home, rtrim($server->docroot, '/') . '/')) {
            throw new InvalidArgumentException('SFTP home must stay inside the server docroot.');
        }

        if (!in_array($shell, ['/usr/sbin/nologin', '/bin/bash'], true)) {
            throw new InvalidArgumentException('SFTP shell is not supported.');
        }

        return compact('username', 'home', 'shell');
    }
}
