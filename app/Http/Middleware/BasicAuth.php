<?php

namespace App\Http\Middleware;

use App\Support\AuthTokens;
use Closure;
use Illuminate\Contracts\Auth\Factory as Auth;

class BasicAuth
{
    protected $auth;

    public function __construct(Auth $auth)
    {
        $this->auth = $auth;
    }

    public function handle($request, Closure $next, $guard = null)
    {
        if (
            hash_equals(AuthTokens::basicUsername(), (string) $request->headers->get('PHP_AUTH_USER')) &&
            hash_equals(AuthTokens::basicPassword(), (string) $request->headers->get('PHP_AUTH_PW'))
        ) {
            return $next($request);
        }

        return response()->json([
            'message' => 'Unauthorized',
        ], 401);
    }
}
