<?php

use Illuminate\Database\Migrations\Migration;

return new class extends Migration
{
    public function up(): void
    {
        // Existing installations should migrate data into the corrected fresh-install
        // schema manually before adding foreign keys. New installs are fixed in the
        // original table migrations to avoid destructive in-place changes here.
    }

    public function down(): void
    {
        //
    }
};
