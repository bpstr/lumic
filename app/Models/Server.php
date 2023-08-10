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
        'template',
    ];

    // casts
    protected $casts = [
        'ssl' => 'date',
    ];

    public function databases() {
        return $this->hasMany(Database::class);
    }

    public function getDatabaseAttribute() {
        return $this->databases()->first() ?? new Database();
    }

    public function getDirectoryAttribute() {
        return sprintf(env('DOCROOT_PATH').'/%s/%s', $this->name, $this->path);
    }

    public function getNginxAttribute() {
        // return nginx config file
        return sprintf(getenv('NGINX_ROOT_PATH').'/sites/%s.conf', $this->name);
    }

    public function getStorageUsedAttribute() {
        $root = sprintf(env('DOCROOT_PATH').'/%s', $this->name);
        if (is_dir($root)) {
            return human_file_size(disk_total_space($root));
        }

        return '–';
    }

}
