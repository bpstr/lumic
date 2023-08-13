<?php

return [
    'templates' => [
        'default' => [
            'selected' => true,
            'name' => 'Default PHP Project',
            'hint' => 'Basic Nignx setup to support PHP based projects',
        ],
        'wordpress' => [
            'name' => 'WordPress Configuration',
            'hint' => 'Nginx configuration for WordPress based projects',
        ],

        'drupal' => [
            'name' => 'Drupal Recommended',
            'hint' => 'Drupal specific Nginx configuration',
        ],
        'laravel' => [
            'name' => 'Laravel Configuration',
            'hint' => 'Nginx configuration for Laravel based projects',
        ],
    ],
];
