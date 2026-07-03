<?php

namespace App\Services;

class DatabaseExplorer
{
    public function overview(): array
    {
        $mysqli = @new \mysqli('localhost', getenv('MYSQL_ROOT_USER'), getenv('MYSQL_ROOT_PASS'));
        if ($mysqli->connect_error) {
            throw new \RuntimeException('Unable to connect to the database server.');
        }

        $users = $this->users($mysqli);
        $databases = $this->databases($mysqli);

        $mysqli->close();

        return compact('users', 'databases');
    }

    private function users(\mysqli $mysqli): array
    {
        $result = $mysqli->query('SELECT user, host FROM mysql.user');
        $users = [];
        if ($result) {
            while ($row = $result->fetch_assoc()) {
                $users[] = $row;
            }
            $result->free();
        }

        return $users;
    }

    private function databases(\mysqli $mysqli): array
    {
        $result = $mysqli->query('SHOW DATABASES');
        $databases = [];
        if (!$result) {
            return [];
        }

        while ($row = $result->fetch_assoc()) {
            $name = $row['Database'];
            if (in_array($name, ['information_schema', 'mysql', 'performance_schema'], true)) {
                continue;
            }

            $databases[] = $this->databaseStats($mysqli, $name);
        }
        $result->free();

        return $databases;
    }

    private function databaseStats(\mysqli $mysqli, string $database): array
    {
        $statement = $mysqli->prepare('SELECT SUM(data_length + index_length) AS db_size, COUNT(*) AS table_count FROM information_schema.tables WHERE table_schema = ?');
        $statement->bind_param('s', $database);
        $statement->execute();
        $row = $statement->get_result()->fetch_assoc() ?: [];
        $statement->close();

        $size = $row['db_size'] ?? null;

        return [
            'name' => $database,
            'size' => $size ? round($size / (1024 * 1024), 2) . ' MB' : 'Unknown',
            'tables' => (string) ($row['table_count'] ?? 0),
        ];
    }
}
