<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('ftp_users', function (Blueprint $table) {
            $table->id();
            $table->foreignId('server_id')->constrained('servers')->cascadeOnDelete();
            $table->string('username')->unique();
            $table->string('home');
            $table->string('shell')->default('/usr/sbin/nologin');
            $table->timestamps();
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('ftp_users');
    }
};
