-- Enable pgvector extension
CREATE EXTENSION IF NOT EXISTS vector;

-- Each service gets its own schema for logical isolation.
-- Physically same PG instance, but each service only touches its own schema.
-- When a service needs to scale out, migrate its schema to a separate database.

CREATE SCHEMA IF NOT EXISTS chat;
CREATE SCHEMA IF NOT EXISTS users;
CREATE SCHEMA IF NOT EXISTS project;
CREATE SCHEMA IF NOT EXISTS billing;
CREATE SCHEMA IF NOT EXISTS memory;
CREATE SCHEMA IF NOT EXISTS skill;
