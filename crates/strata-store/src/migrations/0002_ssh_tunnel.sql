-- Schema v2: SSH tunnel settings (no secret: passwords and passphrases live
-- in the keyring).
ALTER TABLE connections ADD COLUMN ssh_host TEXT;
ALTER TABLE connections ADD COLUMN ssh_port INTEGER;
ALTER TABLE connections ADD COLUMN ssh_user TEXT;
ALTER TABLE connections ADD COLUMN ssh_auth TEXT;
ALTER TABLE connections ADD COLUMN ssh_key_path TEXT;
ALTER TABLE connections ADD COLUMN ssh_secret_policy TEXT;
