<?php

/** @var \Laravel\Lumen\Routing\Router $router */

use App\Jobs\ForceSslCertJob;
use App\Jobs\GitDeployJob;
use App\Jobs\ServerSetupJob;
use App\Models\Cronjob;
use App\Models\Database;
use App\Models\DomainAlias;
use App\Models\FtpUser;
use App\Models\Server;
use App\Support\CronInput;
use App\Support\DatabaseInput;
use App\Support\FtpUserInput;
use App\Support\FileBrowser;
use App\Support\AuthTokens;
use App\Support\PhpRuntime;
use App\Support\ServerInput;
use App\Services\DatabaseExplorer;
use Illuminate\Support\Facades\Artisan;
use Illuminate\Support\Facades\DB;
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

$router->post('/login', function () use ($router) {
    $credentials = request()->only(['name', 'pass']);
    if ($credentials['name'] === getenv('ROOT_USER_NAME') &&
        $credentials['pass'] === getenv('ROOT_USER_PASS')) {

        $cookie = new Cookie(
            'auth',
            AuthTokens::sessionToken($credentials['name'], $credentials['pass']),
            time() + 60 * 60 * 12,
            '/',
            null,
            request()->isSecure(),
            true,
            false,
            Cookie::SAMESITE_LAX
        );

        return redirect('/dashboard')->withCookie($cookie);
    }

    return redirect('/');
});

$router->group(['middleware' => 'auth'], function () use ($router) {
    $router->get('/logout', function () {
        return redirect('/')->withCookie(Cookie::create(
            'auth',
            '',
            time() - 3600,
            '/',
            null,
            request()->isSecure(),
            true,
            false,
            Cookie::SAMESITE_LAX
        ));
    });

    $router->get('/dashboard', function () {
        return view('dashboard', [
            'servers' => Server::all(),
            'database_count' => Database::count(),
            'storage_usage' => round((disk_total_space('/') - disk_free_space('/')) /  disk_total_space('/') * 100),
            'total_storage' => human_file_size( disk_total_space('/')),
            'ftp_count' => 0,
        ]);
    });

    $router->get('/explorer', function () {
        try {
            $data = app(DatabaseExplorer::class)->overview();
        } catch (RuntimeException $exception) {
            $data = ['users' => [], 'databases' => [], 'error' => $exception->getMessage()];
        }

        return view('explorer', $data);
    });

    $router->get('/settings', function () use ($router) {
        return view('settings', [
            'servers' => Server::all(),
            'deploy_token' => AuthTokens::basicToken(),
        ]);
    });

    $router->get('/servers/add', function () {
        $server = new Server();
        $phpRuntime = [
            'versions' => PhpRuntime::versions(),
            'extensions' => PhpRuntime::extensions(),
        ];
        return view('servers.form', compact('server', 'phpRuntime') +  ['servers' => Server::all()]);
    });

    $router->post('/servers/add', function () {
        try {
            $payload = ServerInput::createPayload(request());
        } catch (InvalidArgumentException $exception) {
            return redirect('/servers/add')->with('error', $exception->getMessage());
        }

        $server = Server::create($payload);

        $dbname = Str::slug($payload['domain']);
        $dbuser = Str::slug($payload['domain']);
        $dbpass = Str::random(16);

        Database::create([
            'name' => $dbname,
            'server_id' => $server->id,
            'username' => $dbuser,
            'password' => $dbpass,
        ]);

        Artisan::call('nginx:config', compact('server'));
        dispatch(new ServerSetupJob());

        return redirect('/servers/' . $server->id);
    });

    $router->get('/servers/{id}', function ($id) {
        $server = Server::find($id);
        return view('servers.view', compact('server') +  ['servers' => Server::all()]);
    });

    $router->post('/servers/{id}/update', function ($id) {
        $server = Server::find($id);
        try {
            $payload = ServerInput::updatePayload(request());
        } catch (InvalidArgumentException $exception) {
            return redirect('/servers/' . $server->id . '/deploy')->with('error', $exception->getMessage());
        }

        $server->update($payload);
        return redirect('/servers/' . $server->id . '/deploy');
    });

    $router->get('/servers/{id}/db', function ($id) {
        $server = Server::find($id);
        return view('servers.db', compact('server') +  ['servers' => Server::all()]);
    });

    $router->post('/servers/{id}/db', function ($id) {
        $server = Server::find($id);
        try {
            $payload = DatabaseInput::payload(request());
        } catch (InvalidArgumentException $exception) {
            return redirect('/servers/' . $server->id . '/db')->with('error', $exception->getMessage());
        }

        $database = Database::create($payload + ['server_id' => $server->id]);
        Artisan::call('db:create', ['database' => $database->id]);

        return redirect('/servers/' . $server->id . '/db');
    });


    $router->get('/servers/{id}/ftp', function ($id) {
        $server = Server::find($id);
        return view('servers.ftp', compact('server') +  ['servers' => Server::all()]);
    });

    $router->post('/servers/{id}/ftp', function ($id) {
        $server = Server::find($id);
        try {
            $payload = FtpUserInput::payload(request(), $server);
        } catch (InvalidArgumentException $exception) {
            return redirect('/servers/' . $server->id . '/ftp')->with('error', $exception->getMessage());
        }

        $ftpUser = FtpUser::create($payload + ['server_id' => $server->id]);
        Artisan::call('ftp:create-user', ['ftpUser' => $ftpUser->id]);

        return redirect('/servers/' . $server->id . '/ftp');
    });


    $router->get('/servers/{id}/delete', function ($id) {
        $server = Server::find($id);
        $server->delete();
        return redirect('/dashboard');
    });


    $router->get('/servers/{id}/cron', function ($id) {
        $server = Server::find($id);
        return view('servers.cron', compact('server') +  ['servers' => Server::all()]);
    });

    $router->get('/servers/{id}/domains', function ($id) {
        $server = Server::find($id);
        return view('servers.domains', compact('server') + ['servers' => Server::all()]);
    });

    $router->get('/servers/{id}/files', function ($id) {
        $server = Server::find($id);
        try {
            $path = FileBrowser::resolve($server, request()->query('path'));
        } catch (InvalidArgumentException $exception) {
            return redirect('/servers/' . $server->id)->with('error', $exception->getMessage());
        }

        $entries = FileBrowser::entries($path);
        $relativePath = trim((string) request()->query('path'), '/');

        return view('servers.files', compact('server', 'entries', 'relativePath') + ['servers' => Server::all()]);
    });

    $router->post('/servers/{id}/domains', function ($id) {
        $server = Server::find($id);
        $domain = trim((string) request()->input('domain'));
        try {
            ServerInput::assertDomain($domain);
        } catch (InvalidArgumentException $exception) {
            return redirect('/servers/' . $server->id . '/domains')->with('error', $exception->getMessage());
        }

        DomainAlias::firstOrCreate(['server_id' => $server->id, 'domain' => $domain]);

        return redirect('/servers/' . $server->id . '/domains');
    });

    $router->post('/servers/{id}/cron', function ($id) {
        $server = Server::find($id);
        try {
            $payload = CronInput::payload(request());
        } catch (InvalidArgumentException $exception) {
            return redirect('/servers/' . $server->id . '/cron')->with('error', $exception->getMessage());
        }

        Cronjob::create($payload + ['server_id' => $server->id]);

        return redirect('/servers/' . $server->id . '/cron');
    });

    $router->get('/servers/{id}/deploy', function ($id) {
        $server = Server::find($id);
        $deploy_token = AuthTokens::basicToken();
        return view('servers.deploy', compact('server', 'deploy_token') +  ['servers' => Server::all()]);
    });

    $router->get('/servers/{id}/deploy/trigger', function ($id) {
        $server = Server::find($id);
        dispatch(new GitDeployJob($server));
        return redirect('/servers/' . $server->id . '/deploy');
    });

    $router->get('/servers/{id}/renew', function ($id) {
        $server = Server::find($id);
        dispatch(new ForceSslCertJob($server));
        return redirect('/servers/' . $server->id);
    });

    $router->get('/servers/{id}/deploy/logs', function ($id) {
        $server = Server::find($id);
        $contents = 'No logs yet';
        if (is_file($server->deploy_log)) {
            $contents = file_get_contents($server->deploy_log) ?? 'Logs not readable';
        }

        return sprintf('<pre>%s</pre>', $contents);
    });





});

/** INTERNAL API ENDPOINTS  */
$router->group(['middleware' => 'basic', 'prefix' => 'api'], function () use ($router) {
    $router->get('/status', function () {
        return response()->json([
            'memory' => memory_get_usage(),
            'cpu' => getrusage(),
            'disk' => ['free' => disk_free_space('/'), 'total' => disk_total_space('/')],
        ]);
    });

});
