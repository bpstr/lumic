<?php

/** @var \Laravel\Lumen\Routing\Router $router */

use App\Models\Database;
use App\Models\Server;
use Illuminate\Support\Facades\Artisan;
use Illuminate\Support\Str;
use Symfony\Component\HttpFoundation\Cookie;

/*
|--------------------------------------------------------------------------
| Application Routes
|--------------------------------------------------------------------------
|
| Here is where you can register all the routes for an application.
| It is a breeze. Simply tell Lumen the URIs it should respond to
| and give it the Closure to call when that URI is requested.
|
*/

function human_file_size($size, $unit="") {
    if( (!$unit && $size >= 1000000000) || $unit == "GB")
        return number_format($size/1000000000,2)."GB";
    if( (!$unit && $size >= 1000000) || $unit == "MB")
        return number_format($size/1000000,2)."MB";
    if( (!$unit && $size >= 1000) || $unit == "KB")
        return number_format($size/1000,2)."KB";
    return number_format($size)." bytes";
}

$router->get('/', function () use ($router) {
    return view('login');
});

$router->get('/yo', function () use ($router) {
    return view('sample.index');
});

$router->post('/login', function () use ($router) {
    $credentials = request()->only(['name', 'pass']);
    if ($credentials['name'] === getenv('ROOT_USER_NAME') &&
        $credentials['pass'] === getenv('ROOT_USER_PASS')) {

        $pers = hash('sha256', $credentials['name'] . ':' . $credentials['pass'], true, ['salt' => getenv('APP_KEY')]);
        $cookie = new Cookie('auth', $pers);

        return redirect('/dashboard')->withCookie($cookie);
    }

    return redirect('/');
});

$router->group(['middleware' => 'auth'], function () use ($router) {
    $router->get('/dashboard', function () {
        return view('dashboard', [
            'servers' => Server::all(),
            'database_count' => Database::count(),
            'free_storage' => human_file_size( disk_free_space('/')),
            'total_storage' => human_file_size( disk_total_space('/'))
        ]);
    });

    $router->get('/servers/add', function () {
        $server = new Server();
        return view('servers.form', compact('server') +  ['servers' => Server::all()]);
    });

    $router->post('/servers/add', function () {
        if (empty(request()->input('domain'))) {
            return redirect('/servers/add')->with('error', 'Domain is required!');
        }

        // Todo validate input fields (php version)
        $string = preg_replace('/[^a-zA-Z0-9]/', '-', request()->input('domain'));
        $server = Server::create(request()->all() + [
            'name' => $string,
        ]);

        $database = Database::create([
            'name' => Str::slug(request()->input('domain')),
            'server_id' => $server->id,
            'username' => Str::slug(request()->input('domain')),
            'password' => Str::random(16)
        ]);


        // Run artisan commands (later move this to queue)
        Artisan::call('nginx:config', compact('server'));
        Artisan::call('nginx:restart');
//            Artisan::call('php:config');
//            Artisan::call('php:restart');
        if (request()->input('crate_certificate')) {
            Artisan::call('ssl:certificate', compact('server'));
        }
        if (request()->input('create_db_user')) {
            Artisan::call('db:user', compact('database'));
        }
        if (request()->input('create_database')) {
            Artisan::call('db:create', compact('database'));
        }

        Artisan::call('html:templates', compact('server'));




        return redirect('/servers/' . $server->id);
    });

    $router->get('/servers/{id}', function ($id) {
        $server = Server::find($id);
        return view('servers.view', compact('server') +  ['servers' => Server::all()]);
    });

    $router->get('/servers/{id}/db', function ($id) {
        $server = Server::find($id);
        return view('servers.db', compact('server') +  ['servers' => Server::all()]);
    });

    $router->post('/servers/{id}/db', function ($id) {
        $server = Server::find($id);
        Database::create(['name' => Str::slug(request()->input('name')), 'server_id' => $server->id] + request()->all());
        return redirect('/servers/' . $server->id . '/db');
    });


    $router->get('/servers/{id}/ftp', function ($id) {
        $server = Server::find($id);
        return view('servers.ftp', compact('server') +  ['servers' => Server::all()]);
    });


    $router->get('/servers/{id}/cron', function ($id) {
        $server = Server::find($id);
        return view('servers.cron', compact('server') +  ['servers' => Server::all()]);
    });

    $router->get('/servers/{id}/deploy', function ($id) {
        $server = Server::find($id);
        return view('servers.deploy', compact('server') +  ['servers' => Server::all()]);
    });

    /** INTERNAL API ENDPOINTS  */
    $router->get('/status', function () {
        return response()->json([
            'memory' => memory_get_usage(),
            'cpu' => getrusage(),
            'disk' => ['free' => disk_free_space('/'), 'total' => disk_total_space('/')],
        ]);
    });


});
