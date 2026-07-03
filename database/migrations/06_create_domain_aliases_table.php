<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('domain_aliases', function (Blueprint $table) {
            $table->id();
            $table->foreignId('server_id')->constrained('servers')->cascadeOnDelete();
            $table->string('domain');
            $table->timestamps();
            $table->unique(['server_id', 'domain']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('domain_aliases');
    }
};
