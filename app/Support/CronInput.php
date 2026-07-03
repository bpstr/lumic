<?php

namespace App\Support;

use Illuminate\Http\Request;
use InvalidArgumentException;

class CronInput
{
    private const FIELD_PATTERN = '/^(\*|\d{1,2}|\d{1,2}-\d{1,2}|\*\/\d{1,2}|\d{1,2}(,\d{1,2})*)$/';

    public static function payload(Request $request): array
    {
        $input = $request->input('IspCron', []);
        $payload = [
            'command' => trim((string) ($input['command'] ?? $request->input('command'))),
            'minute' => trim((string) ($input['run_min'] ?? $request->input('minute', '*'))),
            'hour' => trim((string) ($input['run_hour'] ?? $request->input('hour', '*'))),
            'day_of_month' => trim((string) ($input['run_mday'] ?? $request->input('day_of_month', '*'))),
            'month' => trim((string) ($input['run_month'] ?? $request->input('month', '*'))),
            'day_of_week' => trim((string) ($input['run_wday'] ?? $request->input('day_of_week', '*'))),
        ];

        if ($payload['command'] === '' || preg_match('/[\x00-\x1F\x7F]/', $payload['command'])) {
            throw new InvalidArgumentException('Cron command is required and must not contain control characters.');
        }

        foreach (['minute', 'hour', 'day_of_month', 'month', 'day_of_week'] as $field) {
            if (!preg_match(self::FIELD_PATTERN, $payload[$field])) {
                throw new InvalidArgumentException('Cron expression contains an invalid field.');
            }
        }

        return $payload;
    }
}
