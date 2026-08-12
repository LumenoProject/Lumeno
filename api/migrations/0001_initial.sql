CREATE TYPE chain_kind AS ENUM ('unit', 'group');
CREATE TYPE unit_kind AS ENUM (
  'basic', 'assault', 'recon', 'command', 'special_ops', 'engineering', 'quartermaster'
);
CREATE TYPE booking_state AS ENUM ('pending', 'fulfilled', 'awaiting_judgment', 'failed', 'allowed');
CREATE TYPE focus_state AS ENUM ('running', 'completion_ready', 'awaiting_judgment', 'succeeded', 'failed');
CREATE TYPE group_run_state AS ENUM ('running', 'awaiting_judgment', 'succeeded', 'failed');
CREATE TYPE judgment_scope AS ENUM ('main', 'auxiliary', 'group_deadline');
CREATE TYPE rsip_node_state AS ENUM ('draft', 'active', 'archived');

CREATE TABLE app_users (
  id uuid PRIMARY KEY,
  oidc_issuer text NOT NULL CHECK (char_length(oidc_issuer) <= 2048),
  oidc_subject text NOT NULL CHECK (char_length(oidc_subject) BETWEEN 1 AND 512),
  display_name text NOT NULL CHECK (char_length(display_name) BETWEEN 1 AND 160),
  email text CHECK (email IS NULL OR char_length(email) <= 320),
  timezone text NOT NULL DEFAULT 'UTC',
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (oidc_issuer, oidc_subject)
);

CREATE TABLE auth_flows (
  state_hash bytea PRIMARY KEY,
  pkce_verifier text NOT NULL,
  nonce text NOT NULL,
  return_path text NOT NULL DEFAULT '/',
  expires_at timestamptz NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE auth_sessions (
  token_hash bytea PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
  oidc_subject text NOT NULL,
  oidc_session_id text,
  csrf_hash bytea NOT NULL,
  idle_timeout_seconds bigint NOT NULL CHECK (idle_timeout_seconds BETWEEN 60 AND 31536000),
  idle_expires_at timestamptz NOT NULL,
  absolute_expires_at timestamptz NOT NULL,
  last_seen_at timestamptz NOT NULL DEFAULT now(),
  created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX auth_sessions_user_idx ON auth_sessions(user_id);
CREATE INDEX auth_sessions_oidc_sid_idx ON auth_sessions(oidc_session_id) WHERE oidc_session_id IS NOT NULL;
CREATE INDEX auth_sessions_oidc_subject_idx ON auth_sessions(oidc_subject);

CREATE TABLE used_logout_tokens (
  jti text PRIMARY KEY,
  expires_at timestamptz NOT NULL
);

CREATE TABLE chains (
  id uuid PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
  logical_id uuid NOT NULL,
  revision integer NOT NULL CHECK (revision > 0),
  previous_revision_id uuid REFERENCES chains(id),
  parent_id uuid REFERENCES chains(id),
  kind chain_kind NOT NULL,
  unit_kind unit_kind,
  name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
  description text NOT NULL DEFAULT '' CHECK (char_length(description) <= 4000),
  position integer NOT NULL DEFAULT 0,
  sacred_trigger text CHECK (sacred_trigger IS NULL OR char_length(sacred_trigger) BETWEEN 1 AND 1000),
  duration_seconds integer CHECK (duration_seconds IS NULL OR duration_seconds BETWEEN 60 AND 86400),
  auxiliary_signal text CHECK (auxiliary_signal IS NULL OR char_length(auxiliary_signal) BETWEEN 1 AND 1000),
  auxiliary_delay_seconds integer CHECK (auxiliary_delay_seconds IS NULL OR auxiliary_delay_seconds BETWEEN 60 AND 86400),
  auxiliary_completion_trigger text CHECK (auxiliary_completion_trigger IS NULL OR char_length(auxiliary_completion_trigger) BETWEEN 1 AND 1000),
  group_time_limit_seconds integer CHECK (group_time_limit_seconds IS NULL OR group_time_limit_seconds BETWEEN 60 AND 604800),
  main_streak integer NOT NULL DEFAULT 0 CHECK (main_streak >= 0),
  auxiliary_streak integer NOT NULL DEFAULT 0 CHECK (auxiliary_streak >= 0),
  total_completions bigint NOT NULL DEFAULT 0 CHECK (total_completions >= 0),
  total_failures bigint NOT NULL DEFAULT 0 CHECK (total_failures >= 0),
  auxiliary_failures bigint NOT NULL DEFAULT 0 CHECK (auxiliary_failures >= 0),
  version bigint NOT NULL DEFAULT 1,
  archived_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (user_id, logical_id, revision),
  CHECK (
    (kind = 'group' AND unit_kind IS NULL AND sacred_trigger IS NULL AND duration_seconds IS NULL)
    OR
    (kind = 'unit' AND unit_kind IS NOT NULL AND sacred_trigger IS NOT NULL AND auxiliary_signal IS NOT NULL AND auxiliary_delay_seconds IS NOT NULL AND auxiliary_completion_trigger IS NOT NULL)
  ),
  CHECK (parent_id IS NULL OR parent_id <> id)
);
CREATE INDEX chains_user_active_idx ON chains(user_id, position, created_at) WHERE archived_at IS NULL;
CREATE INDEX chains_parent_idx ON chains(parent_id, position) WHERE archived_at IS NULL;
CREATE INDEX chains_user_archived_idx ON chains(user_id, archived_at DESC, id DESC)
  WHERE archived_at IS NOT NULL;

CREATE TABLE precedents (
  id uuid PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
  chain_id uuid NOT NULL REFERENCES chains(id) ON DELETE CASCADE,
  scope judgment_scope NOT NULL,
  rule text NOT NULL CHECK (char_length(rule) BETWEEN 1 AND 1000),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (chain_id, scope, rule)
);

CREATE TABLE group_runs (
  id uuid PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
  chain_id uuid NOT NULL REFERENCES chains(id),
  state group_run_state NOT NULL DEFAULT 'running',
  started_at timestamptz NOT NULL DEFAULT now(),
  expires_at timestamptz,
  completed_at timestamptz,
  version bigint NOT NULL DEFAULT 1
);
CREATE UNIQUE INDEX group_runs_one_active_idx ON group_runs(user_id, chain_id)
  WHERE state IN ('running', 'awaiting_judgment');
CREATE INDEX group_runs_expiry_idx ON group_runs(expires_at) WHERE state = 'running' AND expires_at IS NOT NULL;

CREATE TABLE group_run_items (
  id uuid PRIMARY KEY,
  group_run_id uuid NOT NULL REFERENCES group_runs(id) ON DELETE CASCADE,
  chain_id uuid NOT NULL REFERENCES chains(id),
  position integer NOT NULL,
  completed_at timestamptz,
  UNIQUE (group_run_id, position),
  UNIQUE (group_run_id, chain_id)
);

CREATE TABLE bookings (
  id uuid PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
  chain_id uuid NOT NULL REFERENCES chains(id),
  state booking_state NOT NULL DEFAULT 'pending',
  scheduled_at timestamptz NOT NULL DEFAULT now(),
  expires_at timestamptz NOT NULL,
  confirmation_expires_at timestamptz,
  resolved_at timestamptz,
  precedent_id uuid REFERENCES precedents(id),
  version bigint NOT NULL DEFAULT 1,
  CHECK (state <> 'awaiting_judgment' OR confirmation_expires_at IS NOT NULL)
);
CREATE UNIQUE INDEX bookings_one_pending_idx ON bookings(user_id, chain_id)
  WHERE state IN ('pending', 'awaiting_judgment');
CREATE INDEX bookings_expiry_idx ON bookings(expires_at) WHERE state = 'pending';
CREATE INDEX bookings_confirmation_expiry_idx ON bookings(confirmation_expires_at)
  WHERE state = 'awaiting_judgment';

CREATE TABLE focus_sessions (
  id uuid PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
  chain_id uuid NOT NULL REFERENCES chains(id),
  booking_id uuid REFERENCES bookings(id),
  group_run_id uuid REFERENCES group_runs(id),
  group_run_item_id uuid REFERENCES group_run_items(id),
  state focus_state NOT NULL DEFAULT 'running',
  started_at timestamptz NOT NULL DEFAULT now(),
  completion_available_at timestamptz,
  resolved_at timestamptz,
  pending_action text CHECK (pending_action IS NULL OR pending_action IN ('interrupt', 'early_end')),
  pending_reason text CHECK (pending_reason IS NULL OR char_length(pending_reason) <= 4000),
  version bigint NOT NULL DEFAULT 1
);
CREATE UNIQUE INDEX focus_sessions_one_active_idx ON focus_sessions(user_id)
  WHERE state IN ('running', 'completion_ready', 'awaiting_judgment');
CREATE INDEX focus_sessions_completion_idx ON focus_sessions(completion_available_at)
  WHERE state = 'running' AND completion_available_at IS NOT NULL;

CREATE TABLE protocol_events (
  id uuid PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
  chain_id uuid REFERENCES chains(id),
  aggregate_type text NOT NULL,
  aggregate_id uuid NOT NULL,
  event_type text NOT NULL,
  payload jsonb NOT NULL DEFAULT '{}'::jsonb,
  idempotency_key uuid,
  occurred_at timestamptz NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (user_id, idempotency_key)
);
CREATE INDEX protocol_events_user_time_idx ON protocol_events(user_id, recorded_at DESC, id DESC);
CREATE INDEX protocol_events_user_type_idx ON protocol_events(user_id, event_type);

CREATE TABLE notification_jobs (
  id uuid PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
  event_key text NOT NULL,
  title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 160),
  subtitle text CHECK (subtitle IS NULL OR char_length(subtitle) <= 160),
  body text NOT NULL CHECK (char_length(body) BETWEEN 1 AND 4000),
  target_path text NOT NULL DEFAULT '/' CHECK (char_length(target_path) BETWEEN 1 AND 2048),
  due_at timestamptz NOT NULL,
  attempts integer NOT NULL DEFAULT 0,
  next_attempt_at timestamptz NOT NULL,
  sent_at timestamptz,
  failed_at timestamptz,
  cancelled_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (user_id, event_key)
);
CREATE INDEX notification_jobs_due_idx ON notification_jobs(next_attempt_at)
  WHERE sent_at IS NULL AND failed_at IS NULL AND cancelled_at IS NULL;

CREATE TABLE notification_channels (
  user_id uuid PRIMARY KEY REFERENCES app_users(id) ON DELETE CASCADE,
  kind text NOT NULL CHECK (kind IN ('bark', 'webhook')),
  config_ciphertext bytea NOT NULL CHECK (octet_length(config_ciphertext) BETWEEN 17 AND 8192),
  config_nonce bytea NOT NULL CHECK (octet_length(config_nonce) = 12),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE negative_states (
  id uuid PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
  title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 160),
  description text NOT NULL DEFAULT '' CHECK (char_length(description) <= 4000),
  cues text NOT NULL DEFAULT '' CHECK (char_length(cues) <= 4000),
  consequences text NOT NULL DEFAULT '' CHECK (char_length(consequences) <= 4000),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE traceback_steps (
  id uuid PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
  negative_state_id uuid NOT NULL REFERENCES negative_states(id) ON DELETE CASCADE,
  traceback_id uuid NOT NULL,
  position integer NOT NULL,
  decision text NOT NULL CHECK (char_length(decision) BETWEEN 1 AND 1000),
  is_intervention_point boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (traceback_id, position)
);
CREATE INDEX traceback_steps_negative_state_idx ON traceback_steps(negative_state_id, created_at);

CREATE TABLE rsip_nodes (
  id uuid PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
  negative_state_id uuid REFERENCES negative_states(id) ON DELETE SET NULL,
  intervention_step_id uuid REFERENCES traceback_steps(id) ON DELETE SET NULL,
  parent_id uuid REFERENCES rsip_nodes(id),
  state rsip_node_state NOT NULL DEFAULT 'draft',
  title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 160),
  trigger_condition text NOT NULL CHECK (char_length(trigger_condition) BETWEEN 1 AND 1000),
  required_action text NOT NULL CHECK (char_length(required_action) BETWEEN 1 AND 1000),
  intended_prevention text NOT NULL DEFAULT '' CHECK (char_length(intended_prevention) <= 4000),
  activated_on date,
  activated_at timestamptz,
  archived_at timestamptz,
  failure_reason text CHECK (failure_reason IS NULL OR char_length(failure_reason) <= 4000),
  version bigint NOT NULL DEFAULT 1,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CHECK (parent_id IS NULL OR parent_id <> id),
  CHECK ((state = 'active' AND activated_on IS NOT NULL AND activated_at IS NOT NULL) OR state <> 'active')
);
CREATE UNIQUE INDEX rsip_daily_activation_idx ON rsip_nodes(user_id, activated_on)
  WHERE activated_on IS NOT NULL;
CREATE INDEX rsip_parent_active_idx ON rsip_nodes(parent_id) WHERE state = 'active';

CREATE TABLE rsip_events (
  id uuid PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
  node_id uuid REFERENCES rsip_nodes(id) ON DELETE SET NULL,
  event_type text NOT NULL,
  payload jsonb NOT NULL DEFAULT '{}'::jsonb,
  occurred_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX rsip_events_user_time_idx ON rsip_events(user_id, occurred_at DESC, id DESC);
