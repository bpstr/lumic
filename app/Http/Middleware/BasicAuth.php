<?php

namespace App\Http\Middleware;

use Closure;
use Illuminate\Contracts\Auth\Factory as Auth;

class BasicAuth
{
    /**
     * The authentication guard factory instance.
     *
     * @var \Illuminate\Contracts\Auth\Factory
     */
    protected $auth;

    /**
     * Create a new middleware instance.
     *
     * @param  \Illuminate\Contracts\Auth\Factory  $auth
     * @return void
     */
    public function __construct(Auth $auth)
    {
        $this->auth = $auth;
    }

    /**
     * Handle an incoming request.
     *
     * @param  \Illuminate\Http\Request  $request
     * @param  \Closure  $next
     * @param  string|null  $guard
     * @return mixed
     */
    public function handle($request, Closure $next, $guard = null)
    {
        $username = hash('sha256', getenv('ROOT_USER_NAME'), true, ['salt' => getenv('APP_KEY')]);
        $password = hash('sha256', getenv('ROOT_USER_PASS'), true, ['salt' => getenv('APP_KEY')]);

        if ($username === request()->headers->get('PHP_AUTH_USER') && $password === request()->headers->get('PHP_AUTH_PW')) {
            return $next($request);
        }

        return response()->json([
            'message' => 'Unauthorized'
        ], 401);


//
//        $pers = hash('sha256', getenv('ROOT_USER_NAME') . ':' . getenv('ROOT_USER_PASS'), true, ['salt' => getenv('APP_KEY')]);
//        if(request()->cookies->get('auth') === $pers) {
//        }
//
//        if (request()->wantsJson() || request()->segment(1) === 'api') {
//
//        }
//
//        return redirect('/', 302, [
//            'Cache-Control' => 'no-store, no-cache, must-revalidate, post-check=0, pre-check=0',
//            'Pragma' => 'no-cache'
//        ]);

    }
}
//curl http://143.42.55.228/api/status -H "Authorization: Basic SBNJTRN+FjG7owHVrKtue7eqdM4RhdRWVl71HXN2d7I6zlymc9E7NhGNVKfPE66wygEjg793HnE0IbTR/YQfU5o="
