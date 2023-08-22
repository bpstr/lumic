<?php

/** @var \Laravel\Lumen\Routing\Router $router */

use App\Jobs\ForceSslCertJob;
use App\Jobs\GitDeployJob;
use App\Jobs\ServerSetupJob;
use App\Models\Database;
use App\Models\Server;
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
            'storage_usage' => round((disk_total_space('/') - disk_free_space('/')) /  disk_total_space('/') * 100),
            'total_storage' => human_file_size( disk_total_space('/')),
            'ftp_count' => 0,
        ]);
    });

    $router->get('/explorer',
        function () use ($router) {
            $rootuser = getenv('MYSQL_ROOT_USER');
            $rootpass = getenv('MYSQL_ROOT_PASS');

            // Connect to the MySQL server
            $mysqli = new mysqli('localhost', $rootuser, $rootpass);

            // Check the connection
            if ($mysqli->connect_error) {
                die('Connect Error (' . $mysqli->connect_errno . ') ' . $mysqli->connect_error);
            }

            // Query for users
            $query_users = "SELECT user, host FROM mysql.user";
            $result_users = $mysqli->query($query_users);

            $users = [];
            if ($result_users) {
                while ($row = $result_users->fetch_assoc()) {
                    $users[] = $row;
                }
                $result_users->free();
            }
            $query_dbs = "SHOW DATABASES";
            $result_dbs = $mysqli->query($query_dbs);

            $databases = [];
            if ($result_dbs) {
                while ($row = $result_dbs->fetch_assoc()) {
                    if (in_array($row['Database'], ['information_schema', 'mysql', 'performance_schema'])) {
                        continue;
                    }

                    $databases[] = $row['Database'];
                }
                $result_dbs->free();
            }

            $db_sizes = [];
            $db_table_counts = [];
            foreach ($databases as $db) {
                // Select the database
                $mysqli->select_db($db);

                // Query for database size
                $query_size = "SELECT table_schema AS db_name, SUM(data_length + index_length) AS db_size FROM information_schema.tables WHERE table_schema = '$db' GROUP BY table_schema";
                $result_size = $mysqli->query($query_size);
                if ($result_size) {
                    $row = $result_size->fetch_assoc();
                    $db_sizes[$db] = $row['db_size'] ?? '';
                    $result_size->free();
                }

                // Query for number of tables
                $query_table_count = "SELECT COUNT(*) AS table_count FROM information_schema.tables WHERE table_schema = '$db'";
                $result_table_count = $mysqli->query($query_table_count);
                if ($result_table_count) {
                    $row = $result_table_count->fetch_assoc();
                    $db_table_counts[$db] = $row['table_count'];
                    $result_table_count->free();
                }
            }

            $mysqli->close();

// Output the database sizes and table counts
            foreach ($databases as $index => $db) {
                $databases[$index] = [
                    'name' => $db,
                    'size' => ($db_sizes[$db] ? round($db_sizes[$db] / (1024 * 1024), 2) . " MB" : "Unknown"),
                    'tables' => ($db_table_counts[$db] ? $db_table_counts[$db] : "0"),
                ];
            }

            return view('explorer', compact('users', 'databases'));
        });

    $router->get('/settings', function () use ($router) {
        return view('settings', [
            'servers' => Server::all(),
            'deploy_token' => 'asd',
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

        $dbname = Str::slug(request()->input('domain'));
        $dbuser = Str::slug(request()->input('domain'));
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
        $server->update(request()->all());
        return redirect('/servers/' . $server->id . '/deploy');
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


    $router->get('/servers/{id}/delete', function ($id) {
        $server = Server::find($id);
        $server->delete();
        return redirect('/dashboard');
    });


    $router->get('/servers/{id}/cron', function ($id) {
        $server = Server::find($id);
        return view('servers.cron', compact('server') +  ['servers' => Server::all()]);
    });

    $router->get('/servers/{id}/deploy', function ($id) {
        $server = Server::find($id);
        $username = hash('sha256', getenv('ROOT_USER_NAME'), true, ['salt' => getenv('APP_KEY')]);
        $password = hash('sha256', getenv('ROOT_USER_PASS'), true, ['salt' => getenv('APP_KEY')]);
        $deploy_token = base64_encode($username . ':' . $password);
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

