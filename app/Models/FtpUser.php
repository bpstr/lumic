<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

class FtpUser extends Model
{
    protected $fillable = ['server_id', 'username', 'home', 'shell'];

    public function server()
    {
        return $this->belongsTo(Server::class);
    }
}
