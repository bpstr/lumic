# Task 0010: Add domain alias management

Status: Completed

## Type
Incomplete feature

## Evidence
- README marks "Manage domain aliases (server names)" incomplete.
- The server model stores only one `domain`.
- SSL generation assumes exactly the base domain and `www.` prefix.
- Nginx templates are rendered from a single server object with no alias collection.

## Scope
- Add a domain aliases data model or structured column.
- Validate aliases as domains.
- Render aliases into Nginx `server_name`.
- Include aliases in certificate requests where appropriate.
- Provide UI for adding/removing aliases from a server.

## Acceptance Criteria
- A server can have multiple validated aliases.
- Nginx config and Certbot domain lists include aliases.
- Tests cover alias validation, template rendering, and certificate argument construction.
