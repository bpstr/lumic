<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

class DomainAlias extends Model
{
    protected $fillable = ['server_id', 'domain'];

    public function server()
    {
        return $this->belongsTo(Server::class);
    }
}
