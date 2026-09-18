-- ABOUTME: Separate database roles for domain state and Pi sessions (§20.2), created at first start.
-- ABOUTME: Passwords are set from the mounted secret files by the accompanying shell step.
\set domain_password `cat /run/secrets/memory_domain_password`
\set session_password `cat /run/secrets/memory_session_password`
CREATE ROLE memory_domain LOGIN PASSWORD :'domain_password';
CREATE ROLE memory_session LOGIN PASSWORD :'session_password';
CREATE SCHEMA IF NOT EXISTS pi_sessions AUTHORIZATION memory_session;
GRANT CONNECT ON DATABASE memory TO memory_domain, memory_session;
GRANT ALL ON SCHEMA public TO memory_domain;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT ALL ON TABLES TO memory_domain;
