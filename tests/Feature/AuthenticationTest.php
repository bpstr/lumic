<?php

namespace Tests\Feature;

use App\Support\AuthTokens;
use Tests\TestCase;

class AuthenticationTest extends TestCase
{
    public function test_successful_login_sets_auth_cookie(): void
    {
        $this->post('/login', ['name' => 'root', 'pass' => 'secret']);

        $this->assertResponseStatus(302);
        $this->seeCookie('auth');
    }

    public function test_failed_login_does_not_set_auth_cookie(): void
    {
        $this->post('/login', ['name' => 'root', 'pass' => 'wrong']);

        $this->assertResponseStatus(302);
        $this->dontSeeCookie('auth');
    }

    public function test_protected_page_redirects_without_cookie(): void
    {
        $this->get('/dashboard');

        $this->assertResponseStatus(302);
    }

    public function test_api_unauthorized_response(): void
    {
        $this->get('/api/status');

        $this->assertResponseStatus(401);
        $this->assertStringContainsString('Unauthorized', $this->response->getContent());
    }

    public function test_settings_token_matches_basic_auth_strategy(): void
    {
        $this->assertSame(base64_encode(AuthTokens::basicUsername() . ':' . AuthTokens::basicPassword()), AuthTokens::basicToken());
    }
}
