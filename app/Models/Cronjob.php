<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

class Cronjob extends Model
{
    protected $fillable = [
        'server_id',
        'command',
        'minute',
        'hour',
        'day_of_month',
        'month',
        'day_of_week',
    ];

    public function server()
    {
        return $this->belongsTo(Server::class);
    }

    public function expression(): string
    {
        return implode(' ', [
            $this->minute,
            $this->hour,
            $this->day_of_month,
            $this->month,
            $this->day_of_week,
        ]);
    }
}
