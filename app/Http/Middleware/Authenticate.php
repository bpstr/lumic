<?php

namespace App\Http\Middleware;

use App\Support\AuthTokens;
use Closure;
use Illuminate\Contracts\Auth\Factory as Auth;

class Authenticate
{
    protected $auth;

    public function __construct(Auth $auth)
    {
        $this->auth = $auth;
    }

    public function handle($request, Closure $next, $guard = null)
    {
        if (hash_equals(AuthTokens::sessionToken(), (string) $request->cookies->get('auth'))) {
            return $next($request);
        }

        if ($request->wantsJson() || $request->segment(1) === 'api') {
            return response()->json([
                'message' => 'Unauthorized',
            ], 401);
        }

        return redirect('/', 302, [
            'Cache-Control' => 'no-store, no-cache, must-revalidate, post-check=0, pre-check=0',
            'Pragma' => 'no-cache',
        ]);
    }
}
