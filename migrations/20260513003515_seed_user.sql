-- Add migration script here
INSERT INTO users (user_id, username, password_hash)
VALUES (
    '39147e85-9f53-4076-ac1c-a1e76f1722c7',
    'admin',
    '$argon2id$v=19$m=15000,t=2,p=1$+9ngxxSvTRGCYRb1dvSZMw$T0YRxPN0IpHumBYfajfg0tPrK5NEM1eVx7KlHVNUlkg'
);
