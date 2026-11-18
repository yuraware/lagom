import sqlite3


def get_user(conn: sqlite3.Connection, username: str):
    cursor = conn.execute(f"SELECT id, email FROM users WHERE name = '{username}'")
    return cursor.fetchone()
