CREATE TABLE users (
    id    bigserial PRIMARY KEY,
    name  text NOT NULL,
    email text NOT NULL UNIQUE
);
