<?php

namespace App\Models;

use Illuminate\Auth\Authenticatable;
use Illuminate\Contracts\Auth\Access\Authorizable as AuthorizableContract;
use Illuminate\Contracts\Auth\Authenticatable as AuthenticatableContract;
use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Support\Facades\Artisan;
use Laravel\Lumen\Auth\Authorizable;

class Server extends Model
{
    /**
     * The attributes that are mass assignable.
     *
     * @var string[]
     */
    protected $fillable = [
        'domain',
        'name',
        'path',
        'ssl',
        'php',
        'git',
        'branch',
        'commit',
        'template',
        'setup_status',
        'setup_log',
    ];

    // casts
    protected $casts = [
        'ssl' => 'date',
    ];

    public function appendSetupLog(string $message): void
    {
        $line = '[' . date('Y-m-d H:i:s') . '] ' . $message;
        $this->forceFill([
            'setup_log' => trim(($this->setup_log ? $this->setup_log . PHP_EOL : '') . $line),
        ])->save();
    }

    public function databases() {
        return $this->hasMany(Database::class);
    }

    public function cronjobs() {
        return $this->hasMany(Cronjob::class);
    }

    public function ftpUsers() {
        return $this->hasMany(FtpUser::class);
    }

    public function aliases() {
        return $this->hasMany(DomainAlias::class);
    }

    public function getServerNamesAttribute(): array {
        return array_values(array_unique(array_merge(
            [$this->domain, 'www.' . $this->domain],
            $this->aliases->pluck('domain')->all()
        )));
    }

    public function getDatabaseAttribute() {
        return $this->databases()->first() ?? new Database();
    }

    public function getGitrootAttribute() {
        return sprintf(env('GITROOT_PATH').'/%s/%s', $this->name, $this->path);
    }

    public function getDocrootAttribute() {
        return sprintf(env('DOCROOT_PATH').'/%s/%s', $this->name, $this->path);
    }

    public function getDirectoryAttribute() {
        return sprintf(env('DOCROOT_PATH').'/%s/%s', $this->name, $this->path);
    }

    public function getNginxAttribute() {
        // return nginx config file
        return sprintf(getenv('NGINX_ROOT_PATH').'/sites-enabled/%s.conf', $this->name);
    }

    public function getDeployLogAttribute() {
        return sprintf(env('DOCROOT_PATH').'/%s/deploy.log', $this->name);
    }

    public function getPhpFpmSocketAttribute() {
        return \App\Support\PhpRuntime::socket($this->php ?: '8.1');
    }
}
